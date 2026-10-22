//! Calendar engine (spec `specs/002-calendar`). The calendars of the mail
//! accounts already set up in Settings → Mail: Google Calendar for Gmail,
//! Microsoft Graph for Outlook / Microsoft 365, CalDAV for iCloud and other
//! providers. One OS thread with its own tokio runtime, like the mail engine:
//! per account a task that syncs a window around today every few minutes or
//! when woken, into a store the main thread reads directly. Writes (create,
//! edit, delete, RSVP) go to the provider first and then re-sync.
//!
//! The provider is the truth; the store is a cache kept on disk so the pane
//! and the meeting alerts work at once on launch and while offline.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};

use crate::calendar_time::{self as t, Date};
use crate::config::{MailAccount, MailAuth, MailProvider};

/// How often each account re-syncs while nothing wakes it.
const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);
/// How often a subscribed feed is fetched again.
const FEED_EVERY: Duration = Duration::from_secs(30 * 60);
/// Days before and after today that every account keeps in sync.
const WINDOW_BACK_DAYS: i64 = 14;
const WINDOW_AHEAD_DAYS: i64 = 62;
/// The longest range one browse request may pull in.
const MAX_EXTRA_DAYS: i64 = 120;
/// Keychain slot (beside the mail password) for a calendar refresh token.
const KEYCHAIN_SERVICE: &str = "octoweb.mail";

// ── Model ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Calendar {
	/// `account/id`.
	pub key: String,
	pub account: String,
	pub id: String,
	pub name: String,
	pub color: String,
	pub primary: bool,
	pub writable: bool,
	/// CalDAV: the collection URL.
	#[serde(default)]
	pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attendee {
	pub email: String,
	pub name: String,
	/// `accepted`, `declined`, `tentative` or `needs`.
	pub status: String,
	pub organizer: bool,
	pub me: bool,
	pub optional: bool,
}

/// One occurrence of an event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalEvent {
	/// `account/calendar id/occurrence id`, unique in the store.
	pub key: String,
	pub account: String,
	/// The calendar's key.
	pub calendar: String,
	/// The provider's id for this occurrence (CalDAV: the resource href).
	pub id: String,
	/// The recurring series this occurrence belongs to, if any.
	#[serde(default)]
	pub series: String,
	pub title: String,
	pub start: i64,
	pub end: i64,
	pub all_day: bool,
	/// All-day events: first day and the day after the last (`YYYY-MM-DD`).
	#[serde(default)]
	pub start_date: String,
	#[serde(default)]
	pub end_date: String,
	#[serde(default)]
	pub location: String,
	#[serde(default)]
	pub description: String,
	/// The event in the provider's web calendar.
	#[serde(default)]
	pub link: String,
	/// Video call link, if one was found.
	#[serde(default)]
	pub join: String,
	#[serde(default)]
	pub attendees: Vec<Attendee>,
	#[serde(default)]
	pub organizer: String,
	/// The user's answer to an invitation, empty when not invited.
	#[serde(default)]
	pub me: String,
	pub busy: bool,
	pub recurring: bool,
	pub editable: bool,
	/// `""`, `ooo` (out of office), `focus`, `birthday`.
	#[serde(default)]
	pub kind: String,
	/// CalDAV: the resource's ETag and the occurrence's RECURRENCE-ID form.
	#[serde(default)]
	pub etag: String,
	#[serde(default)]
	pub recurrence_id: String,
	/// The iCalendar UID, shared by every copy of an invitation: how a mail
	/// invitation finds its event.
	#[serde(default)]
	pub uid: String,
}

impl CalEvent {
	pub fn declined(&self) -> bool {
		self.me == "declined"
	}

	/// Whether this is something to attend at a time: not all-day, not
	/// declined, not a free block.
	pub fn is_meeting(&self) -> bool {
		!self.all_day && !self.declined() && self.kind.is_empty()
	}
}

/// A new event, or an edit of one, from the pane or an agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Draft {
	/// Key of the event being edited; empty for a new one.
	#[serde(default)]
	pub key: String,
	/// Key of the calendar to create it in.
	pub calendar: String,
	pub title: String,
	#[serde(default)]
	pub start: i64,
	#[serde(default)]
	pub end: i64,
	#[serde(default)]
	pub all_day: bool,
	/// All-day: first day and the day after the last.
	#[serde(default)]
	pub start_date: String,
	#[serde(default)]
	pub end_date: String,
	#[serde(default)]
	pub location: String,
	#[serde(default)]
	pub description: String,
	#[serde(default)]
	pub attendees: Vec<String>,
	/// Add a video call (Google Meet, Teams).
	#[serde(default)]
	pub video: bool,
	/// `""`, `daily`, `weekdays`, `weekly`, `monthly`, `yearly`.
	#[serde(default)]
	pub recur: String,
}

impl Draft {
	/// Why it can't be saved, if it can't.
	pub fn invalid(&self) -> Option<&'static str> {
		if self.title.trim().is_empty() {
			return Some("Give the event a title.");
		}
		if self.all_day {
			let (Some(a), Some(b)) = (Date::parse(&self.start_date), Date::parse(&self.end_date)) else {
				return Some("Pick the days.");
			};
			if b <= a {
				return Some("The event ends before it starts.");
			}
		} else if self.end <= self.start {
			return Some("The event ends before it starts.");
		}
		if self.attendees.iter().any(|a| !a.contains('@')) {
			return Some("Guests need email addresses.");
		}
		None
	}
}

/// Where an account's calendars live.
#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
	Google,
	Graph,
	/// CalDAV, starting from `url` (a server root or the collection set).
	Dav {
		url: String,
		user: String,
	},
	/// A subscribed ICS feed: one read-only calendar.
	Feed {
		url: String,
		name: String,
		color: String,
	},
}

#[derive(Clone, PartialEq)]
pub enum Auth {
	Grant(crate::mail_oauth::Client),
	Password(String),
	/// A public feed: no sign-in.
	Public,
	/// OAuth calendar access was never granted: the pane offers Connect.
	Connect,
	/// Can't work as set up; the reason.
	Unusable(String),
}

/// One account's calendar credentials, as the engine runs them.
#[derive(Clone, PartialEq)]
pub struct Source {
	pub key: String,
	pub address: String,
	pub backend: Backend,
	pub auth: Auth,
}

impl std::fmt::Debug for Source {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Source")
			.field("key", &self.key)
			.field("backend", &self.backend)
			.finish_non_exhaustive()
	}
}

/// The OAuth sign-in that grants calendar access, per provider.
pub fn endpoints(provider: MailProvider) -> Option<crate::mail_oauth::Endpoints> {
	let mail = crate::mail_oauth::endpoints(provider)?;
	Some(match provider {
		MailProvider::Gmail => crate::mail_oauth::Endpoints {
			scope: "https://www.googleapis.com/auth/calendar",
			..mail
		},
		_ => crate::mail_oauth::Endpoints {
			scope: "offline_access https://graph.microsoft.com/Calendars.ReadWrite",
			..mail
		},
	})
}

fn token_slot(id: &[u8; 16]) -> String {
	format!("{}.calendar", crate::mail::hex(id))
}

pub fn save_token(id: &[u8; 16], token: &str) -> Result<(), String> {
	security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, &token_slot(id), token.as_bytes())
		.map_err(|e| e.to_string())
}

pub fn forget_token(id: &[u8; 16]) {
	// No stored item is fine — nothing to forget.
	let _ = security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, &token_slot(id));
}

fn token(id: &[u8; 16]) -> Option<String> {
	security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, &token_slot(id))
		.ok()
		.and_then(|b| String::from_utf8(b).ok())
}

/// Where CalDAV starts for a provider; `None` for the OAuth providers.
pub fn dav_url(a: &MailAccount) -> Option<String> {
	let own = a.caldav_url.trim();
	if !own.is_empty() {
		return Some(if own.contains("://") {
			own.to_string()
		} else {
			format!("https://{own}")
		});
	}
	match a.provider {
		MailProvider::Icloud => Some("https://caldav.icloud.com/".into()),
		MailProvider::Privateemail => Some("https://dav.privateemail.com/".into()),
		MailProvider::Other => {
			let host = a.imap_host.trim();
			let domain = a.address.rsplit_once('@').map(|(_, d)| d.trim()).unwrap_or_default();
			let host = if domain.is_empty() { host } else { domain };
			(!host.is_empty()).then(|| format!("https://{host}/"))
		}
		_ => None,
	}
}

/// The calendar side of mail account `a`; `None` when its calendar is off.
pub fn source(a: &MailAccount) -> Option<Source> {
	if !a.calendar {
		return None;
	}
	let key = crate::mail::hex(&a.id);
	let address = a.address.trim().to_ascii_lowercase();
	let oauth = |backend: Backend| {
		let auth = match (endpoints(a.provider), crate::mail::oauth_client(a), token(&a.id)) {
			(Some(endpoints), Some((client_id, client_secret)), Some(refresh_token)) => {
				Auth::Grant(crate::mail_oauth::Client {
					endpoints,
					client_id,
					client_secret,
					refresh_token,
				})
			}
			(Some(_), Some(_), None) => Auth::Connect,
			_ => Auth::Unusable("Calendar needs an OAuth client: set one in Settings → Mail.".into()),
		};
		Source {
			key: key.clone(),
			address: address.clone(),
			backend,
			auth,
		}
	};
	Some(match a.provider {
		MailProvider::Gmail => oauth(Backend::Google),
		MailProvider::Outlook => oauth(Backend::Graph),
		_ => {
			let url = dav_url(a)?;
			let user = if a.login.trim().is_empty() {
				address.clone()
			} else {
				a.login.trim().to_string()
			};
			let auth = match crate::mail::password(&a.id) {
				Some(pw) if a.auth == MailAuth::Password => Auth::Password(pw),
				_ => Auth::Unusable("Calendar needs the account's password.".into()),
			};
			Source {
				key,
				address,
				backend: Backend::Dav { url, user },
				auth,
			}
		}
	})
}

/// The key a feed's calendar runs under, in place of an account key.
pub fn feed_key(f: &crate::config::CalendarFeed) -> String {
	format!("feed{}", crate::mail::hex(&f.id))
}

/// `webcal://` and `webcals://` are HTTPS; anything else must already be HTTP(S).
pub fn feed_url(url: &str) -> Option<String> {
	let url = url.trim();
	let lower = url.to_ascii_lowercase();
	for scheme in ["webcals://", "webcal://"] {
		if lower.starts_with(scheme) {
			return Some(format!("https://{}", &url[scheme.len()..]));
		}
	}
	(lower.starts_with("https://") || lower.starts_with("http://")).then(|| url.to_string())
}

/// Colours for feeds that have not picked one, in order.
const FEED_COLORS: [&str; 6] = ["#8e8e93", "#30b0c7", "#a2845e", "#5856d6", "#34c759", "#ff2d55"];

pub fn feed_color(f: &crate::config::CalendarFeed, index: usize) -> String {
	if f.color.is_empty() {
		FEED_COLORS[index % FEED_COLORS.len()].to_string()
	} else {
		f.color.clone()
	}
}

/// Every calendar the engine runs: the mail accounts' and the feeds.
pub fn sources(cfg: &crate::config::Config) -> Vec<Source> {
	let mut out: Vec<Source> = cfg.mail_accounts.iter().filter_map(source).collect();
	for (i, f) in cfg.calendar_feeds.iter().enumerate() {
		if let Some(url) = feed_url(&f.url) {
			out.push(Source {
				key: feed_key(f),
				address: String::new(),
				backend: Backend::Feed {
					url,
					name: f.name.trim().to_string(),
					color: feed_color(f, i),
				},
				auth: Auth::Public,
			});
		}
	}
	out
}

/// Keys of the calendars workspace `ws` shows: its accounts' and feeds.
pub fn keys_in(cfg: &crate::config::Config, ws: &str) -> Vec<String> {
	cfg.mail_accounts
		.iter()
		.filter(|a| a.calendar && a.scope.includes(ws))
		.map(|a| crate::mail::hex(&a.id))
		.chain(cfg.calendar_feeds.iter().filter(|f| f.scope.includes(ws)).map(feed_key))
		.collect()
}

/// Keys whose meetings alert and join: in at least one of `workspaces`.
pub fn alert_keys(cfg: &crate::config::Config, workspaces: &[String]) -> Vec<String> {
	let mut keys: Vec<String> = workspaces.iter().flat_map(|w| keys_in(cfg, w)).collect();
	keys.sort();
	keys.dedup();
	keys
}

// ── Video call links ──────────────────────────────────────────────────────────

/// The URLs in `text`, with `&amp;` decoded and trailing punctuation dropped.
fn urls(text: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut rest = text;
	while let Some(i) = ["https://", "http://", "zoommtg://"]
		.iter()
		.filter_map(|p| rest.find(p))
		.min()
	{
		let tail = &rest[i..];
		let end = tail
			.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`' | '\\'))
			.unwrap_or(tail.len());
		let url = tail[..end]
			.replace("&amp;", "&")
			.trim_end_matches(['.', ',', ';', ':', ')', ']', '!', '?'])
			.to_string();
		out.push(url);
		rest = &tail[end..];
	}
	out
}

fn query_param(url: &str, name: &str) -> Option<String> {
	let query = url.split_once('?')?.1;
	query.split('&').find_map(|p| {
		let (k, v) = p.split_once('=')?;
		(k == name).then(|| percent_decode(v))
	})
}

fn percent_decode(s: &str) -> String {
	let b = s.as_bytes();
	let mut out = Vec::with_capacity(b.len());
	let mut i = 0;
	while i < b.len() {
		if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
			if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
				out.push(v);
				i += 3;
				continue;
			}
		}
		out.push(if b[i] == b'+' { b' ' } else { b[i] });
		i += 1;
	}
	String::from_utf8_lossy(&out).into_owned()
}

/// Outlook SafeLinks and Google's redirector wrap the real link.
fn unwrap_redirect(url: &str) -> String {
	let host = host(url);
	if host.ends_with("safelinks.protection.outlook.com") {
		if let Some(u) = query_param(url, "url") {
			return u;
		}
	}
	if (host == "www.google.com" || host == "google.com") && url.contains("/url?") {
		if let Some(u) = query_param(url, "q").or_else(|| query_param(url, "url")) {
			return u;
		}
	}
	url.to_string()
}

fn host(url: &str) -> String {
	let after = url.find("://").map_or(0, |i| i + 3);
	let rest = &url[after..];
	let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
	let host = &rest[..end];
	let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
	host.split(':').next().unwrap_or_default().to_ascii_lowercase()
}

fn path(url: &str) -> &str {
	let after = url.find("://").map_or(0, |i| i + 3);
	let rest = &url[after..];
	rest.find('/').map_or("", |i| &rest[i..])
}

/// Which video service `url` joins, if it is a meeting link.
pub fn meeting_service(url: &str) -> Option<&'static str> {
	if url.starts_with("zoommtg://") {
		return Some("Zoom");
	}
	let h = host(url);
	let p = path(url);
	let under = |domain: &str| h == domain || h.ends_with(&format!(".{domain}"));
	if ["zoom.us", "zoom.com", "zoom.com.cn", "zoom.de", "zoomgov.com"]
		.iter()
		.any(|d| under(d))
	{
		let first = p.trim_start_matches('/').split('/').next().unwrap_or_default();
		let meeting = ["j", "my", "w", "s", "webinar", "wc"].contains(&first) || (first.len() <= 2 && p.len() > 4);
		return meeting.then_some("Zoom");
	}
	if h == "meet.google.com" {
		let code = p.trim_start_matches('/').trim_start_matches("_meet/");
		return (code.len() >= 3 && code.chars().next().is_some_and(|c| c.is_ascii_lowercase()))
			.then_some("Google Meet");
	}
	if under("teams.microsoft.com") || under("teams.live.com") || h == "teams.microsoft.us" {
		return (p.starts_with("/l/meetup-join/") || p.starts_with("/meet/") || p.starts_with("/l/meet/"))
			.then_some("Teams");
	}
	if under("webex.com") {
		return (p.contains("/j.php") || p.starts_with("/meet/") || p.starts_with("/join/") || p.contains("/meet/"))
			.then_some("Webex");
	}
	let rules: [(&str, &str, &'static str); 13] = [
		("app.slack.com", "/huddle/", "Slack huddle"),
		("around.co", "/", "Around"),
		("whereby.com", "/", "Whereby"),
		("meet.jit.si", "/", "Jitsi"),
		("gotomeeting.com", "/", "GoTo"),
		("meet.goto.com", "/", "GoTo"),
		("gotowebinar.com", "/", "GoTo"),
		("chime.aws", "/", "Chime"),
		("facetime.apple.com", "/join", "FaceTime"),
		("tuple.app", "/c/", "Tuple"),
		("app.gather.town", "/app/", "Gather"),
		("meetings.ringcentral.com", "/j/", "RingCentral"),
		("discord.gg", "/", "Discord"),
	];
	rules
		.iter()
		.find(|(domain, prefix, _)| under(domain) && p.starts_with(prefix) && p.len() > prefix.len())
		.map(|(_, _, name)| *name)
}

/// The video call link among `texts`, searched in order (conference data,
/// URL, location, notes): the first service found wins, and of its links the
/// longest, which keeps Zoom's `pwd=`.
pub fn find_join(texts: &[&str]) -> String {
	for text in texts {
		let mut best: Option<(&'static str, String)> = None;
		for url in urls(text).iter().map(|u| unwrap_redirect(u)) {
			if let Some(service) = meeting_service(&url) {
				match &best {
					Some((s, b)) if *s != service || b.len() >= url.len() => {}
					_ => best = Some((service, url)),
				}
			}
		}
		if let Some((_, url)) = best {
			return url;
		}
	}
	String::new()
}

/// The RRULE for a draft's repeat choice.
pub fn rrule(recur: &str) -> Option<&'static str> {
	Some(match recur {
		"daily" => "FREQ=DAILY",
		"weekdays" => "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
		"weekly" => "FREQ=WEEKLY",
		"monthly" => "FREQ=MONTHLY",
		"yearly" => "FREQ=YEARLY",
		_ => return None,
	})
}

/// Attendee status in the store's words.
pub fn status_word(s: &str) -> String {
	match s.to_ascii_lowercase().as_str() {
		"accepted" | "organizer" => "accepted",
		"declined" => "declined",
		"tentative" | "tentativelyaccepted" => "tentative",
		_ => "needs",
	}
	.to_string()
}

// ── Availability ──────────────────────────────────────────────────────────────

/// Free stretches of at least `min_len` seconds between `day_start` and
/// `day_end` (minutes of the day) on each of `days` local days from `from`,
/// around the busy, undeclined events. Weekends are skipped.
pub fn free_slots(
	events: &[CalEvent],
	from: Date,
	days: i64,
	day_start: u32,
	day_end: u32,
	min_len: i64,
) -> Vec<(i64, i64)> {
	let now = t::now();
	let mut out = Vec::new();
	for i in 0..days {
		let date = from.add_days(i);
		if matches!(date.weekday(), 0 | 6) {
			continue;
		}
		let mut cursor = t::local_ts(date, day_start).max(now - now % 900 + if now % 900 == 0 { 0 } else { 900 });
		let end = t::local_ts(date, day_end);
		let mut busy: Vec<(i64, i64)> = events
			.iter()
			.filter(|e| e.busy && !e.all_day && !e.declined() && e.end > cursor && e.start < end)
			.map(|e| (e.start, e.end))
			.collect();
		busy.sort();
		for (s, e) in busy {
			if s - cursor >= min_len {
				out.push((cursor, s));
			}
			cursor = cursor.max(e);
		}
		if end - cursor >= min_len {
			out.push((cursor, end));
		}
	}
	out
}

fn hm(minute: u32) -> String {
	format!("{}:{:02}", minute / 60, minute % 60)
}

/// Free slots as text to paste into a message: one line per day.
pub fn availability_text(slots: &[(i64, i64)], zone: &str) -> String {
	const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
	const MONTHS: [&str; 12] = [
		"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
	];
	let mut lines: Vec<(Date, Vec<String>)> = Vec::new();
	for (s, e) in slots {
		let (date, sm) = t::local(*s);
		let (_, em) = t::local(*e);
		let range = format!("{}–{}", hm(sm), hm(em));
		match lines.last_mut() {
			Some((d, ranges)) if *d == date => ranges.push(range),
			_ => lines.push((date, vec![range])),
		}
	}
	if lines.is_empty() {
		return String::new();
	}
	let mut out = format!("I'm free at these times ({zone}):\n");
	for (d, ranges) in lines {
		out.push_str(&format!(
			"• {} {} {}: {}\n",
			DAYS[d.weekday() as usize],
			d.d,
			MONTHS[d.m as usize - 1],
			ranges.join(", ")
		));
	}
	out
}

// ── Store ─────────────────────────────────────────────────────────────────────

#[derive(Default, Serialize, Deserialize)]
struct Disk {
	calendars: Vec<Calendar>,
	events: Vec<CalEvent>,
}

/// Calendars and occurrences per account, kept on disk between runs.
pub struct Store {
	path: Option<std::path::PathBuf>,
	calendars: Vec<Calendar>,
	events: HashMap<String, Vec<CalEvent>>,
}

impl Store {
	pub fn open(path: std::path::PathBuf) -> Self {
		let disk: Disk = std::fs::read(&path)
			.ok()
			.and_then(|b| serde_json::from_slice(&b).ok())
			.unwrap_or_default();
		let mut events: HashMap<String, Vec<CalEvent>> = HashMap::new();
		for e in disk.events {
			events.entry(e.account.clone()).or_default().push(e);
		}
		Self {
			path: Some(path),
			calendars: disk.calendars,
			events,
		}
	}

	#[cfg(test)]
	pub fn in_memory() -> Self {
		Self {
			path: None,
			calendars: Vec::new(),
			events: HashMap::new(),
		}
	}

	fn save(&self) {
		let Some(path) = &self.path else {
			return;
		};
		let disk = Disk {
			calendars: self.calendars.clone(),
			events: self.events.values().flatten().cloned().collect(),
		};
		let tmp = path.with_extension("json.tmp");
		let written = serde_json::to_vec(&disk)
			.map_err(|e| e.to_string())
			.and_then(|b| std::fs::write(&tmp, b).map_err(|e| e.to_string()))
			.and_then(|()| std::fs::rename(&tmp, path).map_err(|e| e.to_string()));
		if let Err(e) = written {
			tracing::warn!(error = %e, "calendar cache not saved");
		}
	}

	/// Replace `account`'s calendars; true when they changed.
	fn set_calendars(&mut self, account: &str, mut calendars: Vec<Calendar>) -> bool {
		calendars.sort_by(|a, b| {
			b.primary
				.cmp(&a.primary)
				.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
		});
		let old: Vec<&Calendar> = self.calendars.iter().filter(|c| c.account == account).collect();
		if old.len() == calendars.len() && old.iter().zip(&calendars).all(|(a, b)| *a == b) {
			return false;
		}
		let keys: HashSet<String> = calendars.iter().map(|c| c.key.clone()).collect();
		self.calendars.retain(|c| c.account != account);
		self.calendars.extend(calendars);
		if let Some(events) = self.events.get_mut(account) {
			events.retain(|e| keys.contains(&e.calendar));
		}
		true
	}

	/// Replace what `calendar` holds overlapping `[from, to)`; true on change.
	fn set_range(&mut self, account: &str, calendar: &str, from: i64, to: i64, mut fresh: Vec<CalEvent>) -> bool {
		let events = self.events.entry(account.to_string()).or_default();
		let inside = |e: &CalEvent| e.calendar == calendar && e.end > from && e.start < to;
		let mut old: Vec<CalEvent> = events.iter().filter(|e| inside(e)).cloned().collect();
		fresh.retain(|e| e.end > from && e.start < to);
		old.sort_by(|a, b| a.key.cmp(&b.key));
		fresh.sort_by(|a, b| a.key.cmp(&b.key));
		fresh.dedup_by(|a, b| a.key == b.key);
		if old == fresh {
			return false;
		}
		events.retain(|e| !inside(e));
		events.extend(fresh);
		true
	}

	fn prune(&mut self, account: &str, keep: &[(i64, i64)]) {
		if let Some(events) = self.events.get_mut(account) {
			events.retain(|e| keep.iter().any(|(from, to)| e.end > *from && e.start < *to));
		}
	}

	fn forget(&mut self, account: &str) {
		self.calendars.retain(|c| c.account != account);
		self.events.remove(account);
	}

	pub fn calendars(&self, accounts: &[String]) -> Vec<Calendar> {
		self.calendars
			.iter()
			.filter(|c| accounts.contains(&c.account))
			.cloned()
			.collect()
	}

	pub fn calendar(&self, key: &str) -> Option<Calendar> {
		self.calendars.iter().find(|c| c.key == key).cloned()
	}

	/// The user's copy of the event with iCalendar `uid` among `accounts`:
	/// the occurrence starting at `near`, else the next to come, else any.
	pub fn find_uid(&self, accounts: &[String], uid: &str, near: i64) -> Option<CalEvent> {
		if uid.is_empty() {
			return None;
		}
		let now = t::now();
		let copies: Vec<&CalEvent> = accounts
			.iter()
			.filter_map(|a| self.events.get(a))
			.flatten()
			.filter(|e| e.uid == uid)
			.collect();
		copies
			.iter()
			.find(|e| e.start == near)
			.or_else(|| copies.iter().filter(|e| e.end > now).min_by_key(|e| e.start))
			.or(copies.first())
			.map(|e| (*e).clone())
	}

	pub fn event(&self, key: &str) -> Option<CalEvent> {
		self.events.values().flatten().find(|e| e.key == key).cloned()
	}

	/// Occurrences of `accounts` overlapping `[from, to)`, by start time,
	/// leaving out calendars in `hidden`.
	pub fn events(&self, accounts: &[String], from: i64, to: i64, hidden: &[String]) -> Vec<CalEvent> {
		let mut out: Vec<CalEvent> = accounts
			.iter()
			.filter_map(|a| self.events.get(a))
			.flatten()
			.filter(|e| e.end > from && e.start < to && !hidden.contains(&e.calendar))
			.cloned()
			.collect();
		out.sort_by(|a, b| {
			b.all_day
				.cmp(&a.all_day)
				.then(a.start.cmp(&b.start))
				.then(a.end.cmp(&b.end))
				.then_with(|| a.title.cmp(&b.title))
		});
		// One meeting on two of the user's calendars shows once.
		let mut seen = HashSet::new();
		out.retain(|e| e.all_day || seen.insert((e.title.clone(), e.start, e.end, e.join.clone())));
		out
	}
}

// ── Page ──────────────────────────────────────────────────────────────────────

/// Messages from the calendar page (`calendar_html.rs`).
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
	Ready,
	Hide,
	Fullscreen,
	/// Switch the pane to `notes` or `mail`.
	Tab {
		tab: String,
	},
	Focus,
	Blur,
	Resize {
		width: u32,
	},
	ResizeEnd {
		width: u32,
	},
	ResizeReset,
	/// The occurrences of `[from, to)` for the view.
	Range {
		from: i64,
		to: i64,
	},
	Refresh,
	/// Quick-add text to read into a draft for `calendar`.
	Quick {
		id: u64,
		text: String,
		#[serde(default)]
		calendar: String,
	},
	Save {
		op: u64,
		draft: Draft,
	},
	Delete {
		op: u64,
		key: String,
		all: bool,
	},
	Rsvp {
		op: u64,
		key: String,
		response: String,
		/// The whole series, not this occurrence.
		#[serde(default)]
		series: bool,
	},
	/// Choose an `.ics` file to import.
	ImportPick,
	/// An `.ics` file dropped on the pane.
	ImportText {
		name: String,
		text: String,
	},
	/// Import the file last read into `calendar`.
	Import {
		op: u64,
		calendar: String,
	},
	/// Subscribe this workspace to an ICS feed.
	Subscribe {
		url: String,
		#[serde(default)]
		name: String,
	},
	/// Drop a subscribed feed (its key).
	Unsubscribe {
		account: String,
	},
	Join {
		url: String,
	},
	OpenUrl {
		url: String,
	},
	/// Grant calendar access for an OAuth account.
	Connect {
		account: String,
	},
	Hidden {
		calendars: Vec<String>,
	},
	DefaultCalendar {
		key: String,
	},
	/// A meeting note heading in the workspace notes.
	Notes {
		key: String,
	},
	/// Copy free times for the next `days` days.
	Share {
		days: i64,
	},
	Settings,
	Notice {
		text: String,
	},
}

/// Which provider kind an account's calendars use, for the page.
pub fn kind(a: &MailAccount) -> &'static str {
	match a.provider {
		MailProvider::Gmail => "google",
		MailProvider::Outlook => "graph",
		_ => "dav",
	}
}

/// `window.__cal.<name>(<args>)`.
pub fn script(name: &str, args: &[serde_json::Value]) -> String {
	let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
	format!("window.__cal && window.__cal.{name}({})", args.join(", "))
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub enum Command {
	/// The accounts to run; the cache of any other is dropped.
	SetSources(Vec<Source>),
	/// Sync now: one account, or all of them.
	Refresh(Option<String>),
	/// Make sure `[from, to)` is in the store (browsing away from today).
	Ensure {
		from: i64,
		to: i64,
	},
	/// OAuth sign-in granting calendar access; answered with `Event::Connected`.
	Connect {
		key: String,
		provider: MailProvider,
		client_id: String,
		client_secret: String,
		hint: String,
	},
	Save {
		op: u64,
		draft: Draft,
	},
	Delete {
		op: u64,
		key: String,
		all: bool,
	},
	Rsvp {
		op: u64,
		key: String,
		response: String,
		series: bool,
	},
	/// Add the events of an `.ics` text to a calendar.
	Import {
		op: u64,
		calendar: String,
		text: String,
	},
}

#[derive(Debug)]
pub enum Event {
	/// The store changed: views and alerts should refresh.
	Changed,
	Status {
		account: String,
		state: &'static str,
		message: String,
	},
	/// Show the provider's sign-in page in a tab.
	OpenUrl(String),
	/// Calendar sign-in finished: the refresh token, or why it failed.
	Connected {
		account: String,
		result: Result<String, String>,
	},
	/// A write finished: what to tell the user, or why it failed.
	Done { op: u64, result: Result<String, String> },
}

/// Why a provider call failed: the sign-in needs renewing, or anything else.
#[derive(Debug)]
pub enum ApiError {
	Auth(String),
	Other(String),
}

impl From<String> for ApiError {
	fn from(e: String) -> Self {
		ApiError::Other(e)
	}
}

impl ApiError {
	fn message(&self) -> &str {
		match self {
			ApiError::Auth(m) | ApiError::Other(m) => m,
		}
	}
}

/// How a request authenticates.
#[derive(Clone)]
pub enum Ctx {
	Bearer(String),
	Basic(String, String),
	/// A public feed.
	None,
}

impl Ctx {
	pub fn apply(&self, req: crate::http::Request) -> crate::http::Request {
		match self {
			Ctx::Bearer(token) => req.bearer(token),
			Ctx::Basic(user, pw) => req.basic(user, pw),
			Ctx::None => req,
		}
	}
}

async fn ctx(src: &Source) -> Result<Ctx, ApiError> {
	match (&src.auth, &src.backend) {
		(Auth::Grant(client), _) => crate::mail_oauth::access_token(client)
			.await
			.map(Ctx::Bearer)
			.map_err(ApiError::Auth),
		(Auth::Password(pw), Backend::Dav { user, .. }) => Ok(Ctx::Basic(user.clone(), pw.clone())),
		(Auth::Public, _) => Ok(Ctx::None),
		(Auth::Connect, _) => Err(ApiError::Auth("Connect your calendar".into())),
		(Auth::Unusable(m), _) => Err(ApiError::Other(m.clone())),
		(Auth::Password(_), _) => Err(ApiError::Other("This account signs in with OAuth".into())),
	}
}

const READ_ONLY: &str = "Subscribed calendars are read-only.";

async fn list_calendars(src: &Source, ctx: &Ctx) -> Result<Vec<Calendar>, ApiError> {
	match &src.backend {
		Backend::Google => crate::calendar_google::calendars(&src.key, ctx).await,
		Backend::Graph => crate::calendar_graph::calendars(&src.key, ctx).await,
		Backend::Dav { url, .. } => crate::calendar_dav::calendars(&src.key, url, ctx).await,
		Backend::Feed { .. } => Err(ApiError::Other(READ_ONLY.into())),
	}
}

async fn list_events(src: &Source, ctx: &Ctx, cal: &Calendar, from: i64, to: i64) -> Result<Vec<CalEvent>, ApiError> {
	match &src.backend {
		Backend::Google => crate::calendar_google::events(&src.address, cal, ctx, from, to).await,
		Backend::Graph => crate::calendar_graph::events(&src.address, cal, ctx, from, to).await,
		Backend::Dav { .. } => crate::calendar_dav::events(&src.address, cal, ctx, from, to).await,
		Backend::Feed { .. } => Err(ApiError::Other(READ_ONLY.into())),
	}
}

struct Shared {
	store: Arc<Mutex<Store>>,
	sources: Mutex<HashMap<String, Source>>,
	wake: Mutex<HashMap<String, Arc<Notify>>>,
	/// A browsed range outside the window, kept in sync until replaced.
	extra: Mutex<Option<(i64, i64)>>,
	sink: Box<dyn Fn(Event) + Send + Sync>,
}

impl Shared {
	fn emit(&self, event: Event) {
		(self.sink)(event);
	}

	fn status(&self, account: &str, state: &'static str, message: impl Into<String>) {
		self.emit(Event::Status {
			account: account.to_string(),
			state,
			message: message.into(),
		});
	}

	fn source(&self, key: &str) -> Option<Source> {
		self.sources.lock().unwrap().get(key).cloned()
	}

	fn wake(&self, account: &str) {
		if let Some(n) = self.wake.lock().unwrap().get(account) {
			n.notify_one();
		}
	}
}

/// The window every account keeps: two weeks back, two months ahead.
pub fn window() -> (i64, i64) {
	let today = t::local_date(t::now());
	(
		t::local_midnight(today.add_days(-WINDOW_BACK_DAYS)),
		t::local_midnight(today.add_days(WINDOW_AHEAD_DAYS)),
	)
}

pub struct Engine {
	tx: mpsc::UnboundedSender<Command>,
	store: Arc<Mutex<Store>>,
}

impl Engine {
	/// Start the engine on its own thread with the cache at `path`. `sink`
	/// receives every [`Event`], from the engine's threads.
	pub fn start(path: std::path::PathBuf, sink: impl Fn(Event) + Send + Sync + 'static) -> Result<Self, String> {
		let store = Arc::new(Mutex::new(Store::open(path)));
		let (tx, rx) = mpsc::unbounded_channel();
		let shared = Arc::new(Shared {
			store: store.clone(),
			sources: Mutex::new(HashMap::new()),
			wake: Mutex::new(HashMap::new()),
			extra: Mutex::new(None),
			sink: Box::new(sink),
		});
		std::thread::Builder::new()
			.name("calendar".into())
			.spawn(move || {
				let runtime = tokio::runtime::Builder::new_multi_thread()
					.worker_threads(2)
					.enable_all()
					.build()
					.expect("calendar runtime");
				runtime.block_on(run(shared, rx));
			})
			.map_err(|e| e.to_string())?;
		Ok(Self { tx, store })
	}

	pub fn send(&self, command: Command) {
		// The engine thread lives as long as the app.
		let _ = self.tx.send(command);
	}

	/// The cache, for reads on the main thread. Hold it briefly.
	pub fn store(&self) -> std::sync::MutexGuard<'_, Store> {
		self.store.lock().unwrap()
	}
}

async fn run(shared: Arc<Shared>, mut rx: mpsc::UnboundedReceiver<Command>) {
	let mut tasks: HashMap<String, (Source, tokio::task::JoinHandle<()>)> = HashMap::new();
	while let Some(command) = rx.recv().await {
		match command {
			Command::SetSources(sources) => {
				let wanted: HashMap<String, Source> = sources.into_iter().map(|s| (s.key.clone(), s)).collect();
				tasks.retain(|key, (source, handle)| {
					let keep = wanted.get(key) == Some(source);
					if !keep {
						handle.abort();
					}
					keep
				});
				let gone: Vec<String> = shared
					.sources
					.lock()
					.unwrap()
					.keys()
					.filter(|k| !wanted.contains_key(*k))
					.cloned()
					.collect();
				if !gone.is_empty() {
					let mut store = shared.store.lock().unwrap();
					for key in &gone {
						store.forget(key);
					}
					store.save();
					drop(store);
					shared.emit(Event::Changed);
				}
				*shared.sources.lock().unwrap() = wanted.clone();
				for (key, source) in wanted {
					if tasks.contains_key(&key) {
						continue;
					}
					let notify = Arc::new(Notify::new());
					shared.wake.lock().unwrap().insert(key.clone(), notify.clone());
					let handle = tokio::spawn(account_task(shared.clone(), source.clone(), notify));
					tasks.insert(key, (source, handle));
				}
			}
			Command::Refresh(account) => match account {
				Some(a) => shared.wake(&a),
				None => {
					let keys: Vec<String> = shared.sources.lock().unwrap().keys().cloned().collect();
					for k in keys {
						shared.wake(&k);
					}
				}
			},
			Command::Ensure { from, to } => {
				let (wf, wt) = window();
				if from >= wf && to <= wt {
					continue;
				}
				let covered = shared.extra.lock().unwrap().is_some_and(|(f, t)| from >= f && to <= t);
				if covered {
					continue;
				}
				let span = (to - from).min(MAX_EXTRA_DAYS * 86_400);
				*shared.extra.lock().unwrap() = Some((from - 7 * 86_400, from + span + 7 * 86_400));
				let keys: Vec<String> = shared.sources.lock().unwrap().keys().cloned().collect();
				for k in keys {
					shared.wake(&k);
				}
			}
			Command::Connect {
				key,
				provider,
				client_id,
				client_secret,
				hint,
			} => {
				let shared = shared.clone();
				tokio::spawn(async move {
					let result = match endpoints(provider) {
						Some(e) => {
							let opener = shared.clone();
							crate::mail_oauth::sign_in(&e, &client_id, &client_secret, &hint, move |url| {
								opener.emit(Event::OpenUrl(url));
							})
							.await
						}
						None => Err("Calendar sign-in works for Google and Microsoft accounts.".into()),
					};
					shared.emit(Event::Connected { account: key, result });
				});
			}
			Command::Save { op, draft } => {
				let shared = shared.clone();
				tokio::spawn(async move {
					let result = save(&shared, draft).await;
					shared.emit(Event::Done { op, result });
				});
			}
			Command::Delete { op, key, all } => {
				let shared = shared.clone();
				tokio::spawn(async move {
					let result = delete(&shared, &key, all).await;
					shared.emit(Event::Done { op, result });
				});
			}
			Command::Rsvp {
				op,
				key,
				response,
				series,
			} => {
				let shared = shared.clone();
				tokio::spawn(async move {
					let result = rsvp(&shared, &key, &response, series).await;
					shared.emit(Event::Done { op, result });
				});
			}
			Command::Import { op, calendar, text } => {
				let shared = shared.clone();
				tokio::spawn(async move {
					let result = import(&shared, &calendar, &text).await;
					shared.emit(Event::Done { op, result });
				});
			}
		}
	}
}

async fn account_task(shared: Arc<Shared>, source: Source, wake: Arc<Notify>) {
	// Public feeds change rarely and their hosts ask to be polled gently.
	let every = if matches!(source.backend, Backend::Feed { .. }) {
		FEED_EVERY
	} else {
		SYNC_EVERY
	};
	loop {
		sync(&shared, &source).await;
		let _ = tokio::time::timeout(every, wake.notified()).await;
	}
}

/// Bring a subscribed feed's calendar up to date: one fetch, then the
/// window and any browsed range from the same text.
async fn sync_feed(shared: &Shared, src: &Source, url: &str, name: &str, color: &str) {
	let key = &src.key;
	let req = crate::http::Request::new("GET", url).header("Accept", "text/calendar, */*");
	let text = match crate::http::send(req).await {
		Ok(r) if r.ok() && r.body.contains("BEGIN:VCALENDAR") => r.body,
		Ok(r) if r.ok() => return shared.status(key, "error", "That address is not a calendar feed."),
		Ok(r) => return shared.status(key, "error", format!("The feed answered {}", r.status)),
		Err(e) => return shared.status(key, "error", e),
	};
	let title = if name.is_empty() {
		crate::calendar_dav::calendar_name(&text).unwrap_or_else(|| host(url))
	} else {
		name.to_string()
	};
	let cal = Calendar {
		key: format!("{key}/feed"),
		account: key.clone(),
		id: "feed".into(),
		name: title,
		color: color.to_string(),
		primary: false,
		writable: false,
		url: url.to_string(),
	};
	let mut changed = shared.store.lock().unwrap().set_calendars(key, vec![cal.clone()]);
	let mut ranges = vec![window()];
	if let Some(extra) = *shared.extra.lock().unwrap() {
		ranges.push(extra);
	}
	for &(from, to) in &ranges {
		let events = crate::calendar_dav::feed_occurrences(&text, url, &cal, from, to);
		changed |= shared.store.lock().unwrap().set_range(key, &cal.key, from, to, events);
	}
	{
		let mut store = shared.store.lock().unwrap();
		store.prune(key, &ranges);
		if changed {
			store.save();
		}
	}
	if changed {
		shared.emit(Event::Changed);
	}
	shared.status(key, "online", String::new());
}

/// Bring one account's calendars and the window (and a browsed range) up to
/// date. Failures keep what the store already has.
async fn sync(shared: &Shared, src: &Source) {
	if let Backend::Feed { url, name, color } = &src.backend {
		return sync_feed(shared, src, url, name, color).await;
	}
	let key = &src.key;
	let ctx = match ctx(src).await {
		Ok(c) => c,
		Err(ApiError::Auth(m)) if matches!(src.auth, Auth::Connect) => return shared.status(key, "connect", m),
		Err(ApiError::Auth(m)) => return shared.status(key, "auth", m),
		Err(ApiError::Other(m)) => return shared.status(key, "off", m),
	};
	let calendars = match list_calendars(src, &ctx).await {
		Ok(c) => c,
		Err(e) => {
			tracing::warn!(account = %key, error = %e.message(), "calendar list failed");
			return match (&e, &src.backend) {
				(ApiError::Auth(m), _) => shared.status(key, "auth", m.clone()),
				// A mail provider without a calendar server stays quiet: the
				// pane lists it as off, Settings says how to point at one.
				(ApiError::Other(m), Backend::Dav { .. }) => shared.status(
					key,
					"off",
					format!("No calendar found ({m}). Set the CalDAV server in Settings → Mail."),
				),
				(ApiError::Other(m), _) => shared.status(key, "error", m.clone()),
			};
		}
	};
	let mut changed = shared.store.lock().unwrap().set_calendars(key, calendars.clone());
	let mut ranges = vec![window()];
	if let Some(extra) = *shared.extra.lock().unwrap() {
		ranges.push(extra);
	}
	let mut failure: Option<String> = None;
	for cal in &calendars {
		for &(from, to) in &ranges {
			match list_events(src, &ctx, cal, from, to).await {
				Ok(events) => {
					changed |= shared.store.lock().unwrap().set_range(key, &cal.key, from, to, events);
				}
				Err(e) => {
					tracing::warn!(account = %key, calendar = %cal.name, error = %e.message(), "calendar sync failed");
					failure.get_or_insert_with(|| format!("{}: {}", cal.name, e.message()));
				}
			}
		}
	}
	{
		let mut store = shared.store.lock().unwrap();
		store.prune(key, &ranges);
		if changed {
			store.save();
		}
	}
	if changed {
		shared.emit(Event::Changed);
	}
	match failure {
		Some(m) => shared.status(key, "error", m),
		None => shared.status(key, "online", String::new()),
	}
}

fn locate(shared: &Shared, calendar_key: &str) -> Result<(Source, Calendar), String> {
	let cal = shared
		.store
		.lock()
		.unwrap()
		.calendar(calendar_key)
		.ok_or("That calendar is no longer available.")?;
	let src = shared.source(&cal.account).ok_or("That account's calendar is off.")?;
	Ok((src, cal))
}

async fn save(shared: &Shared, draft: Draft) -> Result<String, String> {
	if let Some(why) = draft.invalid() {
		return Err(why.into());
	}
	let existing = if draft.key.is_empty() {
		None
	} else {
		Some(
			shared
				.store
				.lock()
				.unwrap()
				.event(&draft.key)
				.ok_or("That event is gone.")?,
		)
	};
	let calendar_key = existing.as_ref().map_or(draft.calendar.clone(), |e| e.calendar.clone());
	let (src, cal) = locate(shared, &calendar_key)?;
	if !cal.writable {
		return Err(format!("{} is read-only.", cal.name));
	}
	let ctx = ctx(&src).await.map_err(|e| e.message().to_string())?;
	let result = match &src.backend {
		Backend::Google => crate::calendar_google::save(&cal, &ctx, &draft, existing.as_ref()).await,
		Backend::Graph => crate::calendar_graph::save(&cal, &ctx, &draft, existing.as_ref()).await,
		Backend::Dav { .. } => crate::calendar_dav::save(&cal, &ctx, &draft, existing.as_ref(), &src.address).await,
		Backend::Feed { .. } => Err(ApiError::Other(READ_ONLY.into())),
	};
	result.map_err(|e| e.message().to_string())?;
	shared.wake(&src.key);
	let invited = !draft.attendees.is_empty() && existing.is_none();
	Ok(match (existing.is_some(), invited) {
		(true, _) => "Event updated".into(),
		(false, true) => "Event created, invitations sent".into(),
		(false, false) => "Event created".into(),
	})
}

async fn delete(shared: &Shared, key: &str, all: bool) -> Result<String, String> {
	let event = shared.store.lock().unwrap().event(key).ok_or("That event is gone.")?;
	let (src, cal) = locate(shared, &event.calendar)?;
	let ctx = ctx(&src).await.map_err(|e| e.message().to_string())?;
	let result = match &src.backend {
		Backend::Google => crate::calendar_google::delete(&cal, &ctx, &event, all).await,
		Backend::Graph => crate::calendar_graph::delete(&ctx, &event, all).await,
		Backend::Dav { .. } => crate::calendar_dav::delete(&ctx, &event, all).await,
		Backend::Feed { .. } => Err(ApiError::Other(READ_ONLY.into())),
	};
	result.map_err(|e| e.message().to_string())?;
	// Gone from the view at once; the sync confirms it.
	{
		let mut store = shared.store.lock().unwrap();
		if let Some(events) = store.events.get_mut(&event.account) {
			events.retain(|e| {
				if all && !event.series.is_empty() {
					e.series != event.series
				} else {
					e.key != event.key
				}
			});
		}
		store.save();
	}
	shared.emit(Event::Changed);
	shared.wake(&src.key);
	Ok(if event.attendees.len() > 1 && event.organizer == src.address {
		"Event deleted, guests notified".into()
	} else {
		"Event deleted".into()
	})
}

/// Answer an invitation: this occurrence, or with `series` the whole series.
async fn rsvp(shared: &Shared, key: &str, response: &str, series: bool) -> Result<String, String> {
	if !matches!(response, "accepted" | "declined" | "tentative") {
		return Err("Unknown answer".into());
	}
	let event = shared.store.lock().unwrap().event(key).ok_or("That event is gone.")?;
	let series = series && !event.series.is_empty();
	let target = if series {
		CalEvent {
			id: event.series.clone(),
			..event.clone()
		}
	} else {
		event.clone()
	};
	let (src, cal) = locate(shared, &event.calendar)?;
	let ctx = ctx(&src).await.map_err(|e| e.message().to_string())?;
	let result = match &src.backend {
		Backend::Google => crate::calendar_google::rsvp(&cal, &ctx, &target, response).await,
		Backend::Graph => crate::calendar_graph::rsvp(&ctx, &target, response).await,
		Backend::Dav { .. } => Err(ApiError::Other(
			"Answer this invitation from your mail or calendar app.".into(),
		)),
		Backend::Feed { .. } => Err(ApiError::Other(READ_ONLY.into())),
	};
	result.map_err(|e| e.message().to_string())?;
	{
		let mut store = shared.store.lock().unwrap();
		if let Some(events) = store.events.get_mut(&event.account) {
			for e in events
				.iter_mut()
				.filter(|e| e.key == event.key || (series && e.series == event.series))
			{
				e.me = response.to_string();
				for a in e.attendees.iter_mut().filter(|a| a.me) {
					a.status = response.to_string();
				}
			}
		}
		store.save();
	}
	shared.emit(Event::Changed);
	shared.wake(&src.key);
	Ok(match response {
		"accepted" => "Accepted".into(),
		"declined" => "Declined".into(),
		_ => "Answered maybe".into(),
	})
}

/// Add the events of an `.ics` file (or a mail invitation) to `calendar_key`.
/// Events the account already has (same UID) are left alone, so importing
/// twice adds nothing.
async fn import(shared: &Shared, calendar_key: &str, text: &str) -> Result<String, String> {
	let (src, cal) = locate(shared, calendar_key)?;
	if !cal.writable {
		return Err(format!("{} is read-only.", cal.name));
	}
	let objects = crate::calendar_dav::split_objects(text);
	if objects.is_empty() {
		return Err("There are no events in it.".into());
	}
	let known: HashSet<String> = shared
		.store
		.lock()
		.unwrap()
		.events
		.get(&src.key)
		.map(|events| events.iter().map(|e| e.uid.clone()).filter(|u| !u.is_empty()).collect())
		.unwrap_or_default();
	let ctx = ctx(&src).await.map_err(|e| e.message().to_string())?;
	let (mut added, mut present, mut failed) = (0usize, 0usize, 0usize);
	let mut first_error: Option<String> = None;
	for (uid, object) in &objects {
		if known.contains(uid) {
			present += 1;
			continue;
		}
		let result = match &src.backend {
			Backend::Dav { .. } => crate::calendar_dav::import(&cal, &ctx, uid, object).await,
			Backend::Google => crate::calendar_google::import(&cal, &ctx, object).await,
			Backend::Graph => crate::calendar_graph::import(&cal, &ctx, object).await,
			Backend::Feed { .. } => Err(ApiError::Other(READ_ONLY.into())),
		};
		match result {
			Ok(true) => added += 1,
			Ok(false) => present += 1,
			Err(e) => {
				failed += 1;
				first_error.get_or_insert_with(|| e.message().to_string());
			}
		}
	}
	shared.wake(&src.key);
	if added == 0 && failed > 0 {
		return Err(first_error.unwrap_or_default());
	}
	let mut text = match added {
		0 => "Nothing new to add".to_string(),
		1 => format!("Added 1 event to {}", cal.name),
		n => format!("Added {n} events to {}", cal.name),
	};
	if present > 0 {
		text.push_str(&format!(", {present} already there"));
	}
	if failed > 0 {
		text.push_str(&format!(", {failed} could not be added"));
	}
	Ok(text)
}

// ── For the page and agents ───────────────────────────────────────────────────

/// The meeting to show or join now: one under way (its first 2/3, or with
/// more than 5 minutes left), else the next to start today within 12 hours.
pub fn next_meeting(events: &[CalEvent], now: i64) -> Option<&CalEvent> {
	let ongoing = events
		.iter()
		.filter(|e| e.is_meeting() && e.start <= now && e.end > now)
		.find(|e| {
			let len = e.end - e.start;
			now - e.start < len * 2 / 3 || e.end - now > 5 * 60
		});
	let upcoming = events
		.iter()
		.filter(|e| e.is_meeting() && e.start > now && e.start - now < 12 * 3600)
		.min_by_key(|e| e.start);
	match (ongoing, upcoming) {
		// A meeting about to start beats the tail of one that is ending.
		(Some(o), Some(u)) if u.start - now <= 5 * 60 && o.end - now <= 10 * 60 => Some(u),
		(Some(o), _) => Some(o),
		(None, u) => u,
	}
}

/// An occurrence as text for an agent: a line with what, when and its key.
pub fn describe(e: &CalEvent, calendars: &[Calendar]) -> String {
	let when = if e.all_day {
		let last = Date::parse(&e.end_date)
			.map(|d| d.add_days(-1).iso())
			.unwrap_or_default();
		if last == e.start_date || last.is_empty() {
			format!("{} (all day)", e.start_date)
		} else {
			format!("{} – {} (all day)", e.start_date, last)
		}
	} else {
		let (d, s) = t::local(e.start);
		let (d2, en) = t::local(e.end);
		if d == d2 {
			format!("{} {}–{}", d.iso(), hm(s), hm(en))
		} else {
			format!("{} {} – {} {}", d.iso(), hm(s), d2.iso(), hm(en))
		}
	};
	let cal = calendars
		.iter()
		.find(|c| c.key == e.calendar)
		.map_or("", |c| c.name.as_str());
	let mut out = format!("- {when} · {} [{cal}] (key: {})", e.title, e.key);
	if !e.location.is_empty() {
		out.push_str(&format!("\n  where: {}", e.location));
	}
	if !e.join.is_empty() {
		out.push_str(&format!("\n  video: {}", e.join));
	}
	if !e.me.is_empty() {
		out.push_str(&format!("\n  your answer: {}", e.me));
	}
	if !e.attendees.is_empty() {
		let people: Vec<String> = e
			.attendees
			.iter()
			.take(12)
			.map(|a| {
				let who = if a.name.is_empty() {
					a.email.clone()
				} else {
					format!("{} <{}>", a.name, a.email)
				};
				format!("{who} ({})", a.status)
			})
			.collect();
		out.push_str(&format!("\n  guests: {}", people.join(", ")));
	}
	out
}

/// What an `.ics` file holds, for the import sheet: how many events, and
/// the first few (title and start). `None` when it holds none.
pub fn import_preview(text: &str) -> Option<(usize, Vec<serde_json::Value>)> {
	let objects = crate::calendar_dav::split_objects(text);
	if objects.is_empty() {
		return None;
	}
	let sample = objects
		.iter()
		.take(5)
		.filter_map(|(_, object)| {
			let events = crate::calendar_dav::parse_ics(object, "");
			let ev = events.iter().find(|e| e.recurrence_id.is_none()).or(events.first())?;
			let start = ev.start.as_ref()?;
			Some(serde_json::json!({
				"title": if ev.summary.trim().is_empty() { "(No title)" } else { ev.summary.trim() },
				"start": start.ts(),
				"all_day": start.kind == crate::calendar_dav::TimeKind::Date,
				"recurring": ev.rrule.is_some(),
			}))
		})
		.collect();
	Some((objects.len(), sample))
}

/// A mail invitation (`text/calendar`) for the mail page's card: what it is,
/// and the user's copy of it among `accounts` when the provider already put
/// it in their calendar (Google and Outlook do), which can then be answered.
pub fn invitation(text: &str, store: &Store, accounts: &[String]) -> Option<serde_json::Value> {
	use crate::calendar_dav::TimeKind;
	let events = crate::calendar_dav::parse_ics(text, "");
	let ev = events.iter().find(|e| e.recurrence_id.is_none()).or(events.first())?;
	let start = ev.start.as_ref()?;
	let all_day = start.kind == TimeKind::Date;
	let s = start.ts();
	let e = match (&ev.end, ev.duration) {
		(Some(end), _) => end.ts(),
		(None, Some(d)) => s + d,
		_ if all_day => s + 86_400,
		_ => s + 3600,
	};
	let end_date = match &ev.end {
		Some(end) if all_day => end.date,
		_ => start.date.add_days(1),
	};
	let method = crate::calendar_dav::method(text);
	Some(serde_json::json!({
		"uid": ev.uid,
		"title": if ev.summary.trim().is_empty() { "(No title)" } else { ev.summary.trim() },
		"start": s,
		"end": e.max(s),
		"all_day": all_day,
		"start_date": if all_day { start.date.iso() } else { String::new() },
		"end_date": if all_day { end_date.iso() } else { String::new() },
		"location": ev.location,
		"organizer": ev.organizer,
		"method": method,
		"cancelled": method == "CANCEL" || ev.cancelled,
		"recurring": ev.rrule.is_some(),
		"join": find_join(&[&ev.conference, &ev.url, &ev.location, &ev.description]),
		"event": store.find_uid(accounts, &ev.uid, s),
	}))
}

/// What agents may ask over MCP. Nothing here creates, changes or answers an
/// event: a draft opens in the pane for the user to save.
#[derive(Debug)]
pub enum McpRequest {
	List {
		from: Option<String>,
		days: Option<i64>,
		query: Option<String>,
	},
	Get {
		key: String,
	},
	FreeTime {
		from: Option<String>,
		days: Option<i64>,
		minutes: Option<i64>,
	},
	Draft {
		text: Option<String>,
		draft: Box<Draft>,
		start: Option<String>,
		end: Option<String>,
	},
}

impl McpRequest {
	/// The request's name, for the MCP audit log: never its content.
	pub fn kind(&self) -> &'static str {
		match self {
			Self::List { .. } => "list",
			Self::Get { .. } => "get",
			Self::FreeTime { .. } => "free_time",
			Self::Draft { .. } => "draft",
		}
	}
}

/// An agent's time: `2026-10-22`, `2026-10-22T15:00` (local) or RFC 3339.
pub fn parse_when(s: &str) -> Option<i64> {
	let s = s.trim();
	let Some((date, time)) = s.split_once(['T', ' ']) else {
		return Date::parse(s).map(t::local_midnight);
	};
	let has_zone = time.contains(['Z', 'z', '+']) || time.rfind('-').is_some();
	if has_zone {
		return t::parse_rfc3339(&format!("{date}T{time}"), "");
	}
	let date = Date::parse(date)?;
	let mut parts = time.split(':');
	let h: u32 = parts.next()?.parse().ok()?;
	let m: u32 = parts.next().unwrap_or("0").parse().ok()?;
	(h < 24 && m < 60).then(|| t::local_ts(date, h * 60 + m))
}

/// Answer the read-only requests from the store, for `accounts`.
pub fn answer(
	store: &Store,
	accounts: &[String],
	hidden: &[String],
	day: (u32, u32),
	req: &McpRequest,
) -> Result<String, String> {
	let calendars = store.calendars(accounts);
	if accounts.is_empty() {
		return Err("No calendars in this workspace: add a mail account with a calendar in Settings → Mail.".into());
	}
	let today = t::local_date(t::now());
	let start_of = |from: &Option<String>| -> Result<Date, String> {
		match from {
			Some(f) if !f.trim().is_empty() => {
				Date::parse(f.trim().get(..10).unwrap_or(f)).ok_or_else(|| format!("Unreadable date: {f}"))
			}
			_ => Ok(today),
		}
	};
	match req {
		McpRequest::List { from, days, query } => {
			let first = start_of(from)?;
			let days = days.unwrap_or(7).clamp(1, 62);
			let events = store.events(
				accounts,
				t::local_midnight(first),
				t::local_midnight(first.add_days(days)),
				hidden,
			);
			let q = query.as_deref().unwrap_or("").trim().to_lowercase();
			let lines: Vec<String> = events
				.iter()
				.filter(|e| {
					q.is_empty()
						|| e.title.to_lowercase().contains(&q)
						|| e.location.to_lowercase().contains(&q)
						|| e.attendees
							.iter()
							.any(|a| a.email.contains(&q) || a.name.to_lowercase().contains(&q))
				})
				.map(|e| describe(e, &calendars))
				.collect();
			let span = format!("{} to {}", first.iso(), first.add_days(days - 1).iso());
			Ok(if lines.is_empty() {
				format!("No events {span} (times are local, {}).", t::local_zone())
			} else {
				format!("Events {span}, local time ({}):\n{}", t::local_zone(), lines.join("\n"))
			})
		}
		McpRequest::Get { key } => {
			let e = store
				.event(key)
				.filter(|e| accounts.contains(&e.account))
				.ok_or("No such event in this workspace")?;
			let mut out = describe(&e, &calendars);
			if !e.description.is_empty() {
				out.push_str(&format!("\n  notes:\n{}", crate::mail::untrusted(&e.description)));
			}
			Ok(out)
		}
		McpRequest::FreeTime { from, days, minutes } => {
			let first = start_of(from)?;
			let days = days.unwrap_or(5).clamp(1, 14);
			let span = days + 2 * (days / 5 + 1);
			let events = store.events(
				accounts,
				t::local_midnight(first),
				t::local_midnight(first.add_days(span)),
				hidden,
			);
			let slots = free_slots(
				&events,
				first,
				span,
				day.0,
				day.1,
				minutes.unwrap_or(30).clamp(5, 480) * 60,
			);
			let text = availability_text(&slots, &t::local_zone());
			Ok(if text.is_empty() {
				"No free time in working hours on those days.".into()
			} else {
				format!(
					"{text}(Working hours {}–{}; busy events and weekends excluded.)",
					hm(day.0),
					hm(day.1)
				)
			})
		}
		McpRequest::Draft { .. } => Err("Drafts open in the pane".into()),
	}
}

/// Remove HTML from a provider's description and keep it short.
pub fn plain(description: &str) -> String {
	let text = if description.contains('<') && description.contains('>') {
		crate::mail_parse::html_to_text(description)
	} else {
		description.to_string()
	};
	let mut out = String::new();
	let mut blank = 0;
	for line in text.lines().map(str::trim_end) {
		if line.trim().is_empty() {
			blank += 1;
			if blank > 1 || out.is_empty() {
				continue;
			}
		} else {
			blank = 0;
		}
		out.push_str(line);
		out.push('\n');
	}
	out.trim_end().chars().take(8000).collect()
}

/// The note heading for a meeting, for the workspace notes.
pub fn notes_entry(e: &CalEvent) -> String {
	let (_, minute) = t::local(e.start);
	let mut out = if e.all_day {
		format!("### {}", e.title)
	} else {
		format!("### {} {}", hm(minute), e.title)
	};
	let people: Vec<&str> = e
		.attendees
		.iter()
		.filter(|a| !a.me)
		.map(|a| {
			if a.name.is_empty() {
				a.email.as_str()
			} else {
				a.name.as_str()
			}
		})
		.take(10)
		.collect();
	if !people.is_empty() {
		out.push_str(&format!("\n\nWith {}", people.join(", ")));
	}
	out.push_str("\n\n- ");
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	pub(crate) fn event(key: &str, start: i64, end: i64) -> CalEvent {
		CalEvent {
			key: key.into(),
			account: "a".into(),
			calendar: "a/c".into(),
			id: key.into(),
			series: String::new(),
			title: key.into(),
			start,
			end,
			all_day: false,
			start_date: String::new(),
			end_date: String::new(),
			location: String::new(),
			description: String::new(),
			link: String::new(),
			join: String::new(),
			attendees: Vec::new(),
			organizer: String::new(),
			me: String::new(),
			busy: true,
			recurring: false,
			editable: true,
			kind: String::new(),
			etag: String::new(),
			recurrence_id: String::new(),
			uid: String::new(),
		}
	}

	#[test]
	fn join_links_come_from_the_best_source_and_unwrap() {
		assert_eq!(
			find_join(&[
				"",
				"Room 4",
				"Join: https://us02web.zoom.us/j/123?pwd=abc. Or https://us02web.zoom.us/j/123"
			]),
			"https://us02web.zoom.us/j/123?pwd=abc"
		);
		assert_eq!(
			find_join(&["https://meet.google.com/abc-defg-hij", "https://zoom.us/j/1"]),
			"https://meet.google.com/abc-defg-hij"
		);
		let safe = "https://eur01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fteams.microsoft.com%2Fl%2Fmeetup-join%2F19%253ameeting%2F0&data=x";
		assert_eq!(
			find_join(&[safe]),
			"https://teams.microsoft.com/l/meetup-join/19%3ameeting/0"
		);
		assert_eq!(
			find_join(&[
				"<a href=\"https://www.google.com/url?q=https://meet.google.com/xyz-abcd-efg&amp;sa=D\">Join</a>"
			]),
			"https://meet.google.com/xyz-abcd-efg"
		);
		assert_eq!(find_join(&["https://zoom.us/pricing", "https://example.com/j/1"]), "");
		assert_eq!(
			meeting_service("https://acme.webex.com/acme/j.php?MTID=m1"),
			Some("Webex")
		);
		assert_eq!(
			meeting_service("https://app.slack.com/huddle/T1/C2"),
			Some("Slack huddle")
		);
		assert_eq!(meeting_service("https://teams.microsoft.com/l/chat/0"), None);
	}

	#[test]
	fn the_store_replaces_ranges_and_dedupes_shared_meetings() {
		let mut s = Store::in_memory();
		assert!(s.set_range("a", "a/c", 0, 100, vec![event("x", 10, 20), event("y", 90, 120)]));
		assert!(
			!s.set_range("a", "a/c", 0, 100, vec![event("y", 90, 120), event("x", 10, 20)]),
			"same set"
		);
		assert!(s.set_range("a", "a/c", 0, 50, vec![]));
		let keys: Vec<String> = s
			.events(&["a".into()], 0, 200, &[])
			.into_iter()
			.map(|e| e.key)
			.collect();
		assert_eq!(keys, ["y"]);
		let mut twin = event("y2", 90, 120);
		twin.title = "y".into();
		twin.calendar = "a/d".into();
		s.set_range("a", "a/d", 0, 200, vec![twin]);
		assert_eq!(s.events(&["a".into()], 0, 200, &[]).len(), 1);
		assert!(s
			.events(&["a".into()], 0, 200, &["a/c".into(), "a/d".into()])
			.is_empty());
		s.prune("a", &[(150, 300)]);
		assert!(s.events(&["a".into()], 0, 300, &[]).is_empty());
	}

	#[test]
	fn the_next_meeting_prefers_what_is_about_to_start() {
		let now = 10_000;
		let mut declined = event("declined", now + 60, now + 600);
		declined.me = "declined".into();
		let ending = event("ending", now - 3000, now + 300);
		let next = event("next", now + 120, now + 1800);
		let later = event("later", now + 3600, now + 7200);
		let events = vec![declined.clone(), ending.clone(), next.clone(), later.clone()];
		assert_eq!(next_meeting(&events, now).map(|e| e.key.as_str()), Some("next"));
		let events = vec![ending.clone(), later.clone()];
		assert_eq!(
			next_meeting(&events, now).map(|e| e.key.as_str()),
			Some("later"),
			"the tail of a meeting is not worth a join"
		);
		let fresh = event("fresh", now - 60, now + 1800);
		assert_eq!(next_meeting(&[fresh, next], now).map(|e| e.key.as_str()), Some("fresh"));
	}

	#[test]
	fn availability_skips_busy_time_and_weekends() {
		// Monday 2 Nov 2027, far enough ahead that "now" never clips it.
		let monday = Date::new(2027, 11, 1);
		let busy = event("m", t::local_ts(monday, 10 * 60), t::local_ts(monday, 11 * 60));
		let slots = free_slots(&[busy], monday.add_days(-2), 3, 9 * 60, 17 * 60, 1800);
		assert_eq!(
			slots,
			vec![
				(t::local_ts(monday, 9 * 60), t::local_ts(monday, 10 * 60)),
				(t::local_ts(monday, 11 * 60), t::local_ts(monday, 17 * 60)),
			]
		);
		let text = availability_text(&slots, "Europe/Berlin");
		assert_eq!(
			text,
			"I'm free at these times (Europe/Berlin):\n• Mon 1 Nov: 9:00–10:00, 11:00–17:00\n"
		);
	}

	#[test]
	fn invitations_find_the_users_copy_by_uid() {
		let ics = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:inv-1@google.com\r\nSUMMARY:Design review\r\n\
DTSTART:20261023T080000Z\r\nDTEND:20261023T090000Z\r\nORGANIZER;CN=Anna:mailto:anna@x.com\r\nLOCATION:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
		let mut store = Store::in_memory();
		let card = invitation(ics, &store, &["a".into()]).unwrap();
		assert_eq!(card["title"], "Design review");
		assert_eq!(card["method"], "REQUEST");
		assert_eq!(card["join"], "https://meet.google.com/abc-defg-hij");
		assert!(card["event"].is_null(), "not in the calendar yet: offer Add");

		let mut copy = event("k", 1_792_742_400, 1_792_746_000);
		copy.uid = "inv-1@google.com".into();
		copy.me = "needs".into();
		store.set_range("a", "a/c", 0, i64::MAX, vec![copy]);
		let card = invitation(ics, &store, &["a".into()]).unwrap();
		assert_eq!(card["event"]["key"], "k");
		assert!(
			invitation(ics, &store, &["other".into()]).unwrap()["event"].is_null(),
			"only this workspace's calendars"
		);

		let cancel = ics.replace("METHOD:REQUEST", "METHOD:CANCEL");
		assert_eq!(invitation(&cancel, &store, &[]).unwrap()["cancelled"], true);
	}

	#[test]
	fn feeds_take_webcal_and_previews_count_events() {
		assert_eq!(feed_url("webcal://x.com/h.ics").as_deref(), Some("https://x.com/h.ics"));
		assert_eq!(feed_url(" WEBCALS://x.com/a ").as_deref(), Some("https://x.com/a"));
		assert_eq!(feed_url("http://x.com/a.ics").as_deref(), Some("http://x.com/a.ics"));
		assert_eq!(feed_url("ftp://x"), None);
		let file = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:1\nDTSTART:20261023T080000Z\nSUMMARY:One\nEND:VEVENT\nBEGIN:VEVENT\nUID:2\nDTSTART;VALUE=DATE:20261024\nSUMMARY:Two\nEND:VEVENT\nEND:VCALENDAR";
		let (n, sample) = import_preview(file).unwrap();
		assert_eq!(n, 2);
		assert_eq!(sample[1]["all_day"], true);
		assert!(import_preview("BEGIN:VCALENDAR\nEND:VCALENDAR").is_none());
	}

	#[test]
	fn page_messages_deserialize() {
		for msg in [
			r#"{"type":"ready"}"#,
			r#"{"type":"range","from":1792627200,"to":1794441600}"#,
			r#"{"type":"quick","id":3,"text":"Lunch tomorrow","calendar":"a/b"}"#,
			r#"{"type":"save","op":1,"draft":{"key":"","calendar":"a/b","title":"x","start":1,"end":2,"all_day":false,"location":"","description":"","attendees":[],"video":false,"recur":"","account":"a","id":"x"}}"#,
			r#"{"type":"delete","op":2,"key":"k","all":false}"#,
			r#"{"type":"rsvp","op":3,"key":"k","response":"accepted"}"#,
			r#"{"type":"hidden","calendars":["a/b"]}"#,
			r#"{"type":"share","days":5}"#,
			r#"{"type":"resize_end","width":400}"#,
		] {
			assert!(serde_json::from_str::<Request>(msg).is_ok(), "{msg}");
		}
	}

	#[test]
	fn drafts_explain_what_is_missing() {
		let mut d = Draft {
			title: "Sync".into(),
			start: 100,
			end: 200,
			..Default::default()
		};
		assert_eq!(d.invalid(), None);
		d.attendees = vec!["anna".into()];
		assert_eq!(d.invalid(), Some("Guests need email addresses."));
		d.attendees.clear();
		d.end = 50;
		assert!(d.invalid().is_some());
		d.all_day = true;
		d.start_date = "2026-10-22".into();
		d.end_date = "2026-10-23".into();
		assert_eq!(d.invalid(), None);
	}

	#[test]
	fn descriptions_lose_markup_and_extra_blank_lines() {
		assert_eq!(plain("<p>Agenda</p><br><br><br>1. Plan"), "Agenda\n\n1. Plan");
		assert_eq!(plain("a\n\n\n\nb"), "a\n\nb");
	}
}
