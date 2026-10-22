//! CalDAV (RFC 4791) for iCloud, Private Email and any other provider:
//! discovery through `/.well-known/caldav` and the principal, a time-range
//! `calendar-query` per calendar, and iCalendar (RFC 5545) both ways.
//! Recurring events are expanded here rather than by the server, which keeps
//! one behaviour across servers and the original time zones intact.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::calendar::{self, ApiError, Attendee, CalEvent, Calendar, Ctx, Draft};
use crate::calendar_time::{self as t, Date};
use crate::http::{self, Request};

// ── XML ───────────────────────────────────────────────────────────────────────

/// An element with namespace prefixes dropped: DAV answers are small and their
/// element names don't clash across the namespaces we read.
#[derive(Debug, Default)]
pub struct Node {
	pub name: String,
	pub attrs: Vec<(String, String)>,
	pub children: Vec<Node>,
	pub text: String,
}

impl Node {
	pub fn child(&self, name: &str) -> Option<&Node> {
		self.children.iter().find(|c| c.name == name)
	}

	/// The first descendant named `name`.
	pub fn find(&self, name: &str) -> Option<&Node> {
		for c in &self.children {
			if c.name == name {
				return Some(c);
			}
			if let Some(n) = c.find(name) {
				return Some(n);
			}
		}
		None
	}

	pub fn all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
		for c in &self.children {
			if c.name == name {
				out.push(c);
			}
			c.all(name, out);
		}
	}

	pub fn attr(&self, name: &str) -> Option<&str> {
		self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
	}

	/// Text of the first descendant named `name`, trimmed.
	pub fn text_of(&self, name: &str) -> Option<String> {
		self.find(name).map(|n| n.text.trim().to_string())
	}
}

fn local_name(name: &str) -> String {
	name.rsplit(':').next().unwrap_or(name).to_string()
}

fn unescape_xml(s: &str) -> String {
	if !s.contains('&') {
		return s.to_string();
	}
	let mut out = String::with_capacity(s.len());
	let mut rest = s;
	while let Some(i) = rest.find('&') {
		out.push_str(&rest[..i]);
		let tail = &rest[i..];
		let Some(end) = tail.find(';').filter(|e| *e < 12) else {
			out.push('&');
			rest = &tail[1..];
			continue;
		};
		let entity = &tail[1..end];
		let ch = match entity {
			"lt" => Some('<'),
			"gt" => Some('>'),
			"amp" => Some('&'),
			"quot" => Some('"'),
			"apos" => Some('\''),
			e if e.starts_with("#x") || e.starts_with("#X") => {
				u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32)
			}
			e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
			_ => None,
		};
		match ch {
			Some(c) => {
				out.push(c);
				rest = &tail[end + 1..];
			}
			None => {
				out.push('&');
				rest = &tail[1..];
			}
		}
	}
	out.push_str(rest);
	out
}

/// Parse a document into its root element.
pub fn parse_xml(doc: &str) -> Option<Node> {
	let mut stack: Vec<Node> = vec![Node::default()];
	let mut rest = doc;
	while !rest.is_empty() {
		let Some(lt) = rest.find('<') else {
			stack.last_mut()?.text.push_str(&unescape_xml(rest));
			break;
		};
		if lt > 0 {
			stack.last_mut()?.text.push_str(&unescape_xml(&rest[..lt]));
		}
		rest = &rest[lt..];
		if let Some(r) = rest.strip_prefix("<![CDATA[") {
			let end = r.find("]]>")?;
			stack.last_mut()?.text.push_str(&r[..end]);
			rest = &r[end + 3..];
		} else if let Some(r) = rest.strip_prefix("<!--") {
			rest = &r[r.find("-->")? + 3..];
		} else if rest.starts_with("<?") || rest.starts_with("<!") {
			rest = &rest[rest.find('>')? + 1..];
		} else if let Some(r) = rest.strip_prefix("</") {
			let end = r.find('>')?;
			rest = &r[end + 1..];
			let node = stack.pop()?;
			stack.last_mut()?.children.push(node);
		} else {
			// A start tag; quoted attribute values may hold `>`.
			let bytes = rest.as_bytes();
			let mut i = 1;
			let mut quote = 0u8;
			while i < bytes.len() {
				match bytes[i] {
					b'"' | b'\'' if quote == 0 => quote = bytes[i],
					q if q == quote => quote = 0,
					b'>' if quote == 0 => break,
					_ => {}
				}
				i += 1;
			}
			if i >= bytes.len() {
				return None;
			}
			let inner = &rest[1..i];
			let self_closing = inner.ends_with('/');
			let inner = inner.trim_end_matches('/');
			let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
			let mut node = Node {
				name: local_name(&inner[..name_end]),
				..Default::default()
			};
			let mut attrs = &inner[name_end..];
			while let Some(eq) = attrs.find('=') {
				let name = attrs[..eq].trim().to_string();
				let after = attrs[eq + 1..].trim_start();
				let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
					break;
				};
				let Some(close) = after[1..].find(q) else {
					break;
				};
				node.attrs.push((local_name(&name), unescape_xml(&after[1..1 + close])));
				attrs = &after[close + 2..];
			}
			rest = &rest[i + 1..];
			if self_closing {
				stack.last_mut()?.children.push(node);
			} else {
				stack.push(node);
			}
		}
	}
	stack.into_iter().next()?.children.into_iter().next()
}

// ── iCalendar ─────────────────────────────────────────────────────────────────

/// A property line: name, parameters, value.
#[derive(Debug, Clone)]
pub struct Prop {
	pub name: String,
	pub params: Vec<(String, String)>,
	pub value: String,
}

impl Prop {
	pub fn param(&self, name: &str) -> Option<&str> {
		self.params
			.iter()
			.find(|(n, _)| n.eq_ignore_ascii_case(name))
			.map(|(_, v)| v.as_str())
	}
}

/// Unfolded content lines.
fn unfold(text: &str) -> Vec<String> {
	let mut out: Vec<String> = Vec::new();
	for line in text.split('\n') {
		let line = line.strip_suffix('\r').unwrap_or(line);
		if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
			out.last_mut().expect("not empty").push_str(&line[1..]);
		} else if !line.is_empty() {
			out.push(line.to_string());
		}
	}
	out
}

pub fn parse_line(line: &str) -> Option<Prop> {
	// The value starts at the first `:` outside a quoted parameter.
	let mut quoted = false;
	let colon = line.char_indices().find(|&(_, c)| {
		if c == '"' {
			quoted = !quoted;
		}
		c == ':' && !quoted
	})?;
	let (head, value) = (&line[..colon.0], &line[colon.0 + 1..]);
	let mut parts = Vec::new();
	let mut current = String::new();
	let mut quoted = false;
	for c in head.chars() {
		match c {
			'"' => quoted = !quoted,
			';' if !quoted => parts.push(std::mem::take(&mut current)),
			c => current.push(c),
		}
	}
	parts.push(current);
	let mut parts = parts.into_iter();
	let name = parts.next()?.to_ascii_uppercase();
	let params = parts
		.filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.to_string())))
		.collect();
	Some(Prop {
		name,
		params,
		value: value.to_string(),
	})
}

fn unescape_text(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut chars = s.chars();
	while let Some(c) = chars.next() {
		if c != '\\' {
			out.push(c);
			continue;
		}
		match chars.next() {
			Some('n' | 'N') => out.push('\n'),
			Some(c) => out.push(c),
			None => {}
		}
	}
	out
}

fn escape_text(s: &str) -> String {
	s.replace('\\', "\\\\")
		.replace(';', "\\;")
		.replace(',', "\\,")
		.replace("\r\n", "\\n")
		.replace('\n', "\\n")
}

/// A DATE or DATE-TIME value and how to read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Time {
	pub date: Date,
	/// Seconds into the day; 0 for dates.
	pub secs: u32,
	pub kind: TimeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimeKind {
	Date,
	Utc,
	Zone(String),
	Floating,
}

impl Time {
	pub fn parse(value: &str, tzid: Option<&str>) -> Option<Self> {
		let value = value.trim();
		let (date, time) = match value.split_once('T') {
			Some((d, t)) => (d, Some(t)),
			None => (value, None),
		};
		let date = Date::parse(date)?;
		let Some(time) = time else {
			return Some(Self {
				date,
				secs: 0,
				kind: TimeKind::Date,
			});
		};
		let utc = time.ends_with('Z') || time.ends_with('z');
		let digits = time.trim_end_matches(['Z', 'z']);
		if digits.len() < 4 {
			return None;
		}
		let h: u32 = digits[..2].parse().ok()?;
		let m: u32 = digits[2..4].parse().ok()?;
		let s: u32 = digits.get(4..6).and_then(|s| s.parse().ok()).unwrap_or(0);
		let kind = match (utc, tzid) {
			(true, _) => TimeKind::Utc,
			(false, Some(z)) if !z.is_empty() => TimeKind::Zone(z.to_string()),
			_ => TimeKind::Floating,
		};
		Some(Self {
			date,
			secs: h * 3600 + m * 60 + s,
			kind,
		})
	}

	fn of(p: &Prop) -> Option<Self> {
		Self::parse(&p.value, p.param("TZID"))
	}

	pub fn ts(&self) -> i64 {
		let minute = self.secs / 60;
		let extra = i64::from(self.secs % 60);
		match &self.kind {
			TimeKind::Date => t::local_midnight(self.date),
			TimeKind::Utc => self.date.days() * 86_400 + i64::from(self.secs),
			TimeKind::Zone(z) => t::zone_ts(z, self.date, minute) + extra,
			TimeKind::Floating => t::local_ts(self.date, minute) + extra,
		}
	}

	/// The same wall time on another day.
	fn on(&self, date: Date) -> Self {
		Self {
			date,
			secs: self.secs,
			kind: self.kind.clone(),
		}
	}

	/// As a property's parameters and value, e.g. `;TZID=X:20261022T100000`.
	pub fn ical(&self) -> String {
		let hms = format!(
			"{:02}{:02}{:02}",
			self.secs / 3600,
			self.secs % 3600 / 60,
			self.secs % 60
		);
		match &self.kind {
			TimeKind::Date => format!(";VALUE=DATE:{}", self.date.compact()),
			TimeKind::Utc => format!(":{}T{hms}Z", self.date.compact()),
			TimeKind::Zone(z) => format!(";TZID={z}:{}T{hms}", self.date.compact()),
			TimeKind::Floating => format!(":{}T{hms}", self.date.compact()),
		}
	}

	fn same_instant(&self, other: &Time) -> bool {
		if self.kind == TimeKind::Date || other.kind == TimeKind::Date {
			self.date == other.date
		} else {
			self.ts() == other.ts()
		}
	}
}

#[derive(Debug, Clone, Default)]
pub struct VEvent {
	pub uid: String,
	pub summary: String,
	pub description: String,
	pub location: String,
	pub url: String,
	pub start: Option<Time>,
	pub end: Option<Time>,
	pub duration: Option<i64>,
	pub rrule: Option<String>,
	pub exdates: Vec<Time>,
	pub rdates: Vec<Time>,
	pub recurrence_id: Option<Time>,
	pub cancelled: bool,
	pub transparent: bool,
	pub organizer: String,
	pub attendees: Vec<Attendee>,
	/// Conference properties some servers add (Google, Teams).
	pub conference: String,
}

fn mailto(v: &str) -> String {
	let v = v.trim();
	let v = v
		.strip_prefix("mailto:")
		.or_else(|| v.strip_prefix("MAILTO:"))
		.unwrap_or(v);
	v.to_ascii_lowercase()
}

/// `P1DT2H30M`-style durations in seconds.
fn duration(v: &str) -> Option<i64> {
	let v = v.trim();
	let (sign, v) = match v.strip_prefix('-') {
		Some(r) => (-1, r),
		None => (1, v.trim_start_matches('+')),
	};
	let v = v.strip_prefix('P')?;
	let mut total = 0i64;
	let mut num = String::new();
	let mut in_time = false;
	for c in v.chars() {
		match c {
			'T' => in_time = true,
			'0'..='9' => num.push(c),
			_ => {
				let n: i64 = num.parse().ok()?;
				num.clear();
				total += n * match (c, in_time) {
					('W', _) => 7 * 86_400,
					('D', _) => 86_400,
					('H', true) => 3600,
					('M', true) => 60,
					('S', true) => 1,
					_ => return None,
				};
			}
		}
	}
	Some(sign * total)
}

/// The VEVENTs of a calendar object; alarms and other nested parts skipped.
pub fn parse_ics(text: &str, address: &str) -> Vec<VEvent> {
	let mut out = Vec::new();
	let mut current: Option<VEvent> = None;
	let mut depth = 0;
	for line in unfold(text) {
		let Some(p) = parse_line(&line) else {
			continue;
		};
		match p.name.as_str() {
			"BEGIN" if p.value.eq_ignore_ascii_case("VEVENT") && current.is_none() => {
				current = Some(VEvent::default());
				depth = 0;
				continue;
			}
			"BEGIN" if current.is_some() => {
				depth += 1;
				continue;
			}
			"END" if current.is_some() && depth > 0 => {
				depth -= 1;
				continue;
			}
			"END" if p.value.eq_ignore_ascii_case("VEVENT") => {
				out.extend(current.take());
				continue;
			}
			_ => {}
		}
		let Some(ev) = current.as_mut().filter(|_| depth == 0) else {
			continue;
		};
		match p.name.as_str() {
			"UID" => ev.uid = p.value.clone(),
			"SUMMARY" => ev.summary = unescape_text(&p.value),
			"DESCRIPTION" => ev.description = unescape_text(&p.value),
			"LOCATION" => ev.location = unescape_text(&p.value),
			"URL" => ev.url = p.value.clone(),
			"DTSTART" => ev.start = Time::of(&p),
			"DTEND" => ev.end = Time::of(&p),
			"DURATION" => ev.duration = duration(&p.value),
			"RRULE" => ev.rrule = Some(p.value.clone()),
			"EXDATE" | "RDATE" => {
				let tz = p.param("TZID");
				let times = p.value.split(',').filter_map(|v| Time::parse(v, tz));
				if p.name == "EXDATE" {
					ev.exdates.extend(times);
				} else {
					ev.rdates.extend(times);
				}
			}
			"RECURRENCE-ID" => ev.recurrence_id = Time::of(&p),
			"STATUS" => ev.cancelled = p.value.eq_ignore_ascii_case("CANCELLED"),
			"TRANSP" => ev.transparent = p.value.eq_ignore_ascii_case("TRANSPARENT"),
			"ORGANIZER" => ev.organizer = mailto(&p.value),
			"ATTENDEE" => {
				if p.param("CUTYPE")
					.is_some_and(|c| c.eq_ignore_ascii_case("ROOM") || c.eq_ignore_ascii_case("RESOURCE"))
				{
					continue;
				}
				let email = mailto(&p.value);
				ev.attendees.push(Attendee {
					me: email == address,
					name: p.param("CN").unwrap_or_default().to_string(),
					status: calendar::status_word(p.param("PARTSTAT").unwrap_or("")),
					organizer: false,
					optional: p
						.param("ROLE")
						.is_some_and(|r| r.eq_ignore_ascii_case("OPT-PARTICIPANT")),
					email,
				});
			}
			"X-GOOGLE-CONFERENCE"
			| "X-MICROSOFT-SKYPETEAMSMEETINGURL"
			| "X-MICROSOFT-ONLINEMEETINGCONFLINK"
			| "CONFERENCE" => {
				ev.conference.push_str(&p.value);
				ev.conference.push(' ');
			}
			_ => {}
		}
	}
	for ev in &mut out {
		let organizer = ev.organizer.clone();
		for a in ev.attendees.iter_mut().filter(|a| a.email == organizer) {
			a.organizer = true;
		}
	}
	out
}

// ── Recurrence ────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct Rule {
	pub freq: String,
	pub interval: i64,
	pub count: Option<usize>,
	pub until: Option<Time>,
	/// (ordinal, weekday 0 = Sunday); ordinal 0 = every such day.
	pub by_day: Vec<(i32, u32)>,
	pub by_month_day: Vec<i32>,
	pub by_month: Vec<u32>,
	pub by_set_pos: Vec<i32>,
	pub week_start: u32,
}

fn weekday(code: &str) -> Option<u32> {
	["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
		.iter()
		.position(|d| code.eq_ignore_ascii_case(d))
		.map(|i| i as u32)
}

pub fn parse_rule(value: &str) -> Option<Rule> {
	let mut rule = Rule {
		interval: 1,
		week_start: 1,
		..Default::default()
	};
	for part in value.split(';') {
		let (k, v) = part.split_once('=')?;
		let list = || v.split(',').map(str::trim).filter(|s| !s.is_empty());
		match k.to_ascii_uppercase().as_str() {
			"FREQ" => rule.freq = v.to_ascii_uppercase(),
			"INTERVAL" => rule.interval = v.parse().ok().filter(|n: &i64| *n > 0)?,
			"COUNT" => rule.count = v.parse().ok(),
			"UNTIL" => rule.until = Time::parse(v, None),
			"BYDAY" => {
				rule.by_day = list()
					.filter_map(|d| {
						let split = d.len().checked_sub(2)?;
						let ord = if split == 0 {
							0
						} else {
							d[..split].trim_start_matches('+').parse().ok()?
						};
						Some((ord, weekday(&d[split..])?))
					})
					.collect();
			}
			"BYMONTHDAY" => rule.by_month_day = list().filter_map(|d| d.parse().ok()).collect(),
			"BYMONTH" => rule.by_month = list().filter_map(|d| d.parse().ok()).collect(),
			"BYSETPOS" => rule.by_set_pos = list().filter_map(|d| d.parse().ok()).collect(),
			"WKST" => rule.week_start = weekday(v).unwrap_or(1),
			_ => {}
		}
	}
	matches!(rule.freq.as_str(), "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY").then_some(rule)
}

/// Days of month `(y, m)` the rule picks, sorted, `default_day` when it
/// names none.
fn month_days(y: i32, m: u32, rule: &Rule, default_day: u32) -> Vec<Date> {
	let dim = Date::days_in_month(y, m);
	let first = Date::new(y, m, 1);
	let mut days: Vec<u32> = Vec::new();
	if !rule.by_month_day.is_empty() {
		for &n in &rule.by_month_day {
			let d = if n > 0 { n } else { dim as i32 + 1 + n };
			if d >= 1 && d <= dim as i32 {
				days.push(d as u32);
			}
		}
		if !rule.by_day.is_empty() {
			days.retain(|&d| rule.by_day.iter().any(|(_, wd)| Date::new(y, m, d).weekday() == *wd));
		}
	} else if !rule.by_day.is_empty() {
		for &(ord, wd) in &rule.by_day {
			let first_wd = (wd + 7 - first.weekday()) % 7 + 1;
			let all: Vec<u32> = (0..5).map(|k| first_wd + 7 * k).filter(|d| *d <= dim).collect();
			match ord {
				0 => days.extend(&all),
				n if n > 0 => days.extend(all.get(n as usize - 1)),
				n => days.extend(all.len().checked_sub((-n) as usize).and_then(|i| all.get(i))),
			}
		}
	} else if default_day <= dim {
		days.push(default_day);
	}
	days.sort_unstable();
	days.dedup();
	let mut dates: Vec<Date> = days.into_iter().map(|d| Date::new(y, m, d)).collect();
	if !rule.by_set_pos.is_empty() {
		let picked: Vec<Date> = rule
			.by_set_pos
			.iter()
			.filter_map(|&p| {
				let i = if p > 0 {
					p as usize - 1
				} else {
					dates.len().checked_sub((-p) as usize)?
				};
				dates.get(i).copied()
			})
			.collect();
		dates = picked;
		dates.sort();
	}
	dates
}

/// Start dates of a series from `start`, until `limit` (inclusive) or the
/// rule's COUNT. UNTIL is left to the caller, which knows the instants.
pub fn expand_dates(start: Date, rule: &Rule, limit: Date) -> Vec<Date> {
	const MAX_PERIODS: i64 = 20_000;
	let mut out = Vec::new();
	let count = rule.count.unwrap_or(usize::MAX);
	let in_months = |d: &Date| rule.by_month.is_empty() || rule.by_month.contains(&d.m);
	let week_start = start.add_days(-(((start.weekday() + 7 - rule.week_start) % 7) as i64));
	for k in 0..MAX_PERIODS {
		let n = k * rule.interval;
		let mut candidates: Vec<Date> = match rule.freq.as_str() {
			"DAILY" => {
				let d = start.add_days(n);
				let by_day_ok = rule.by_day.is_empty() || rule.by_day.iter().any(|(_, wd)| *wd == d.weekday());
				let by_month_day_ok = rule.by_month_day.is_empty()
					|| rule.by_month_day.iter().any(|&md| {
						let dim = Date::days_in_month(d.y, d.m) as i32;
						(if md > 0 { md } else { dim + 1 + md }) == d.d as i32
					});
				if by_day_ok && by_month_day_ok {
					vec![d]
				} else {
					vec![]
				}
			}
			"WEEKLY" => {
				let ws = week_start.add_days(7 * n);
				let mut days: Vec<Date> = if rule.by_day.is_empty() {
					vec![ws.add_days(((start.weekday() + 7 - rule.week_start) % 7) as i64)]
				} else {
					rule.by_day
						.iter()
						.map(|(_, wd)| ws.add_days(((wd + 7 - rule.week_start) % 7) as i64))
						.collect()
				};
				days.sort();
				days.dedup();
				days
			}
			"MONTHLY" => {
				let first = Date::new(start.y, start.m, 1).add_months(n as i32);
				month_days(first.y, first.m, rule, start.d)
			}
			_ => {
				let y = start.y + n as i32;
				let months: Vec<u32> = if rule.by_month.is_empty() {
					vec![start.m]
				} else {
					rule.by_month.clone()
				};
				let mut all: Vec<Date> = months.iter().flat_map(|&m| month_days(y, m, rule, start.d)).collect();
				all.sort();
				all
			}
		};
		candidates.retain(in_months);
		let period_start = candidates.first().copied();
		for d in candidates {
			if d < start {
				continue;
			}
			if d > limit || out.len() >= count {
				return out;
			}
			out.push(d);
		}
		// Periods past the limit can't add anything.
		let past = match rule.freq.as_str() {
			"DAILY" => start.add_days(n) > limit,
			"WEEKLY" => week_start.add_days(7 * n) > limit,
			"MONTHLY" => Date::new(start.y, start.m, 1).add_months(n as i32) > limit,
			_ => start.y + n as i32 > limit.y,
		};
		if past || (period_start.is_some_and(|p| p > limit)) {
			return out;
		}
	}
	out
}

/// One occurrence of a VEVENT: its start and end instants and, for a series,
/// its RECURRENCE-ID.
#[derive(Debug, Clone)]
pub struct Instance {
	pub start: i64,
	pub end: i64,
	pub start_date: Option<Date>,
	pub end_date: Option<Date>,
	pub recurrence_id: Option<Time>,
}

fn span(ev: &VEvent) -> (i64, i64) {
	// (seconds for timed events, days for dates)
	let Some(start) = &ev.start else {
		return (0, 0);
	};
	if start.kind == TimeKind::Date {
		let days = match (&ev.end, ev.duration) {
			(Some(end), _) => (end.date.days() - start.date.days()).max(1),
			(None, Some(d)) => (d / 86_400).max(1),
			_ => 1,
		};
		return (0, days);
	}
	let secs = match (&ev.end, ev.duration) {
		(Some(end), _) => end.ts() - start.ts(),
		(None, Some(d)) => d,
		_ => 0,
	};
	(secs.max(0), 0)
}

fn instance(ev: &VEvent, start: &Time, recurrence_id: Option<Time>) -> Instance {
	let (secs, days) = span(ev);
	if start.kind == TimeKind::Date {
		let end = start.date.add_days(days);
		return Instance {
			start: t::local_midnight(start.date),
			end: t::local_midnight(end),
			start_date: Some(start.date),
			end_date: Some(end),
			recurrence_id,
		};
	}
	let s = start.ts();
	Instance {
		start: s,
		end: s + secs,
		start_date: None,
		end_date: None,
		recurrence_id,
	}
}

/// The occurrences of `master` overlapping `[from, to)`, leaving out
/// EXDATEs and those replaced by an override in `overridden`.
pub fn instances(master: &VEvent, overridden: &[Time], from: i64, to: i64) -> Vec<Instance> {
	let Some(start) = &master.start else {
		return Vec::new();
	};
	let Some(rule) = master.rrule.as_deref().and_then(parse_rule) else {
		let i = instance(master, start, None);
		return if i.end > from && i.start < to || (i.start == i.end && i.start >= from && i.start < to) {
			vec![i]
		} else {
			Vec::new()
		};
	};
	let until = rule.until.as_ref().map(|u| match u.kind {
		TimeKind::Date => t::local_midnight(u.date.add_days(1)) - 1,
		_ => u.ts(),
	});
	let mut limit = t::local_date(to).add_days(1);
	if let Some(u) = &rule.until {
		limit = limit.min(u.date.add_days(1));
	}
	let mut starts: Vec<Time> = expand_dates(start.date, &rule, limit)
		.into_iter()
		.map(|d| start.on(d))
		.collect();
	starts.extend(master.rdates.iter().cloned());
	let mut out = Vec::new();
	for s in starts {
		if until.is_some_and(|u| s.ts() > u) {
			continue;
		}
		if master.exdates.iter().chain(overridden).any(|x| x.same_instant(&s)) {
			continue;
		}
		let i = instance(master, &s, Some(s.clone()));
		if i.end > from && i.start < to || (i.start == i.end && i.start >= from && i.start < to) {
			out.push(i);
		}
	}
	out
}

/// Occurrences of one calendar object (all VEVENTs sharing a UID) in range.
pub fn occurrences(
	text: &str,
	href: &str,
	etag: &str,
	cal: &Calendar,
	address: &str,
	from: i64,
	to: i64,
) -> Vec<CalEvent> {
	let events = parse_ics(text, address);
	let overrides: Vec<Time> = events.iter().filter_map(|e| e.recurrence_id.clone()).collect();
	let series = events.iter().any(|e| e.rrule.is_some() || e.recurrence_id.is_some());
	let mut out = Vec::new();
	for ev in &events {
		if ev.cancelled {
			continue;
		}
		let list = if ev.recurrence_id.is_some() {
			let mut one = ev.clone();
			one.rrule = None;
			instances(&one, &[], from, to)
				.into_iter()
				.map(|mut i| {
					i.recurrence_id = ev.recurrence_id.clone();
					i
				})
				.collect()
		} else {
			instances(ev, &overrides, from, to)
		};
		for i in list {
			out.push(to_event(ev, i, href, etag, cal, address, series));
		}
	}
	out
}

fn to_event(ev: &VEvent, i: Instance, href: &str, etag: &str, cal: &Calendar, address: &str, series: bool) -> CalEvent {
	let rid = i.recurrence_id.as_ref().map(Time::ical).unwrap_or_default();
	let me = ev
		.attendees
		.iter()
		.find(|a| a.me)
		.map(|a| a.status.clone())
		.unwrap_or_default();
	let mine = ev.organizer.is_empty() || ev.organizer == address;
	CalEvent {
		key: format!("{}/{href}#{rid}", cal.key),
		account: cal.account.clone(),
		calendar: cal.key.clone(),
		id: href.to_string(),
		series: if series { href.to_string() } else { String::new() },
		title: if ev.summary.trim().is_empty() {
			"(No title)".into()
		} else {
			ev.summary.trim().to_string()
		},
		start: i.start,
		end: i.end,
		all_day: i.start_date.is_some(),
		start_date: i.start_date.map(Date::iso).unwrap_or_default(),
		end_date: i.end_date.map(Date::iso).unwrap_or_default(),
		location: ev.location.clone(),
		description: calendar::plain(&ev.description),
		link: if ev.url.starts_with("http") {
			ev.url.clone()
		} else {
			String::new()
		},
		join: calendar::find_join(&[&ev.conference, &ev.url, &ev.location, &ev.description]),
		attendees: ev.attendees.clone(),
		organizer: ev.organizer.clone(),
		me: if mine { String::new() } else { me },
		busy: !ev.transparent,
		recurring: series,
		editable: cal.writable && mine && !series,
		kind: String::new(),
		etag: etag.to_string(),
		recurrence_id: rid,
		uid: ev.uid.clone(),
	}
}

// ── Files and feeds ───────────────────────────────────────────────────────────

/// One calendar object per UID from a file or feed holding many: the
/// calendar's VERSION and PRODID, the VTIMEZONEs its events use, and that
/// UID's VEVENTs (the series and its overrides). METHOD is dropped: CalDAV
/// stores objects, not messages.
pub fn split_objects(text: &str) -> Vec<(String, String)> {
	let mut header: Vec<String> = Vec::new();
	let mut zones: Vec<(String, Vec<String>)> = Vec::new();
	let mut events: Vec<(String, Vec<String>)> = Vec::new();
	let mut block: Vec<String> = Vec::new();
	let mut kind = String::new();
	let mut depth = 0;
	let mut uid = String::new();
	let mut tzid = String::new();
	for line in unfold(text) {
		let p = parse_line(&line);
		let (name, value) = p.as_ref().map_or(("", ""), |p| (p.name.as_str(), p.value.as_str()));
		if depth == 0 {
			if name == "BEGIN" && !value.eq_ignore_ascii_case("VCALENDAR") {
				kind = value.to_ascii_uppercase();
				depth = 1;
				block = vec![line];
				uid.clear();
				tzid.clear();
			} else if !matches!(name, "BEGIN" | "END" | "METHOD") && !name.is_empty() {
				header.push(line);
			}
			continue;
		}
		match name {
			"BEGIN" => depth += 1,
			"END" => depth -= 1,
			"UID" if depth == 1 && uid.is_empty() => uid = value.trim().to_string(),
			"TZID" if depth == 1 && tzid.is_empty() => tzid = value.trim().to_string(),
			_ => {}
		}
		block.push(line);
		if depth > 0 {
			continue;
		}
		match kind.as_str() {
			"VTIMEZONE" => zones.push((tzid.clone(), std::mem::take(&mut block))),
			"VEVENT" => {
				let id = if uid.is_empty() {
					format!("{}@octoweb", crate::mail::hex(&crate::workspace::random_bytes16()))
				} else {
					uid.clone()
				};
				match events.iter_mut().find(|(u, _)| *u == id) {
					Some((_, lines)) => lines.append(&mut block),
					None => events.push((id, std::mem::take(&mut block))),
				}
			}
			_ => block.clear(),
		}
	}
	if !header.iter().any(|l| l.to_ascii_uppercase().starts_with("VERSION")) {
		header.insert(0, "VERSION:2.0".into());
	}
	if !header.iter().any(|l| l.to_ascii_uppercase().starts_with("PRODID")) {
		header.push("PRODID:-//Octoweb//Calendar//EN".into());
	}
	events
		.into_iter()
		.map(|(uid, lines)| {
			let used: Vec<String> = lines
				.iter()
				.filter_map(|l| parse_line(l))
				.filter_map(|p| p.param("TZID").map(|z| z.trim_matches('"').to_string()))
				.collect();
			let mut out = vec!["BEGIN:VCALENDAR".to_string()];
			out.extend(header.iter().cloned());
			for (id, zone) in &zones {
				if used.iter().any(|u| u == id) {
					out.extend(zone.iter().cloned());
				}
			}
			out.extend(lines);
			out.push("END:VCALENDAR".into());
			(uid, out.iter().map(|l| fold(l)).collect())
		})
		.collect()
}

/// The iTIP method of a calendar message (`REQUEST`, `CANCEL`, …), upper case.
pub fn method(text: &str) -> String {
	unfold(text)
		.iter()
		.filter_map(|l| parse_line(l))
		.find(|p| p.name == "METHOD")
		.map(|p| p.value.trim().to_ascii_uppercase())
		.unwrap_or_default()
}

/// A feed's own name (`X-WR-CALNAME`), if it gives one.
pub fn calendar_name(text: &str) -> Option<String> {
	unfold(text)
		.iter()
		.filter_map(|l| parse_line(l))
		.find(|p| p.name == "X-WR-CALNAME")
		.map(|p| unescape_text(&p.value).trim().to_string())
		.filter(|n| !n.is_empty())
}

/// The occurrences of a whole feed in `[from, to)`, keyed by feed and UID.
pub fn feed_occurrences(text: &str, feed: &str, cal: &Calendar, from: i64, to: i64) -> Vec<CalEvent> {
	split_objects(text)
		.into_iter()
		.flat_map(|(uid, object)| occurrences(&object, &format!("{feed}#{uid}"), "", cal, "", from, to))
		.collect()
}

// ── Writing ───────────────────────────────────────────────────────────────────

/// Fold a content line at 75 octets, on character boundaries.
fn fold(line: &str) -> String {
	let mut out = String::new();
	let mut len = 0;
	for c in line.chars() {
		if len + c.len_utf8() > 75 {
			out.push_str("\r\n ");
			len = 1;
		}
		out.push(c);
		len += c.len_utf8();
	}
	out.push_str("\r\n");
	out
}

fn when(draft: &Draft, start: bool, zone: Option<&str>) -> String {
	if draft.all_day {
		let d = if start { &draft.start_date } else { &draft.end_date };
		return format!(";VALUE=DATE:{}", Date::parse(d).map(Date::compact).unwrap_or_default());
	}
	let ts = if start { draft.start } else { draft.end };
	match zone {
		Some(z) => {
			let (date, minute) = t::local(ts);
			format!(";TZID={z}:{}T{:02}{:02}00", date.compact(), minute / 60, minute % 60)
		}
		None => format!(":{}", t::ical_utc(ts)),
	}
}

/// A new calendar object for `draft`. A repeating timed event is written in
/// the Mac's zone so it keeps its wall time across DST, unless `utc`.
pub fn new_ics(uid: &str, draft: &Draft, organizer: &str, utc: bool) -> String {
	let zone = (!utc && !draft.recur.is_empty() && !draft.all_day).then(t::local_zone);
	let mut lines = vec![
		"BEGIN:VCALENDAR".to_string(),
		"VERSION:2.0".into(),
		"PRODID:-//Octoweb//Calendar//EN".into(),
		"BEGIN:VEVENT".into(),
		format!("UID:{uid}"),
		format!("DTSTAMP:{}", t::ical_utc(t::now())),
		format!("DTSTART{}", when(draft, true, zone.as_deref())),
		format!("DTEND{}", when(draft, false, zone.as_deref())),
		format!("SUMMARY:{}", escape_text(draft.title.trim())),
	];
	if !draft.location.trim().is_empty() {
		lines.push(format!("LOCATION:{}", escape_text(draft.location.trim())));
	}
	if !draft.description.trim().is_empty() {
		lines.push(format!("DESCRIPTION:{}", escape_text(&draft.description)));
	}
	if let Some(rule) = calendar::rrule(&draft.recur) {
		lines.push(format!("RRULE:{rule}"));
	}
	if !draft.attendees.is_empty() {
		lines.push(format!("ORGANIZER:mailto:{organizer}"));
		lines.push(format!("ATTENDEE;PARTSTAT=ACCEPTED;ROLE=CHAIR:mailto:{organizer}"));
		for a in &draft.attendees {
			lines.push(format!(
				"ATTENDEE;PARTSTAT=NEEDS-ACTION;ROLE=REQ-PARTICIPANT;RSVP=TRUE:mailto:{}",
				a.trim()
			));
		}
	}
	lines.extend(["END:VEVENT".to_string(), "END:VCALENDAR".into()]);
	lines.iter().map(|l| fold(l)).collect()
}

/// `text` with the (single, non-recurring) VEVENT rewritten from `draft`:
/// what the draft names is replaced, everything else the server or another
/// client put there stays.
pub fn edit_ics(text: &str, draft: &Draft, address: &str, keep_description: bool) -> String {
	let wanted: Vec<String> = draft.attendees.iter().map(|a| a.trim().to_ascii_lowercase()).collect();
	let mut out = Vec::new();
	let mut in_event = false;
	let mut depth = 0;
	let mut seen_attendees: Vec<String> = Vec::new();
	let mut organizer = String::new();
	let mut sequence = 0i64;
	for line in unfold(text) {
		let p = parse_line(&line);
		let name = p.as_ref().map(|p| p.name.clone()).unwrap_or_default();
		let value = p.as_ref().map(|p| p.value.clone()).unwrap_or_default();
		if name == "BEGIN" && value.eq_ignore_ascii_case("VEVENT") {
			in_event = true;
			depth = 0;
			out.push(line);
			continue;
		}
		if in_event && name == "BEGIN" {
			depth += 1;
		}
		if in_event && name == "END" && depth > 0 {
			depth -= 1;
			out.push(line);
			continue;
		}
		if in_event && depth == 0 && name == "END" && value.eq_ignore_ascii_case("VEVENT") {
			out.push(format!("DTSTART{}", when(draft, true, None)));
			out.push(format!("DTEND{}", when(draft, false, None)));
			out.push(format!("SUMMARY:{}", escape_text(draft.title.trim())));
			if !draft.location.trim().is_empty() {
				out.push(format!("LOCATION:{}", escape_text(draft.location.trim())));
			}
			if !keep_description && !draft.description.trim().is_empty() {
				out.push(format!("DESCRIPTION:{}", escape_text(&draft.description)));
			}
			out.push(format!("SEQUENCE:{}", sequence + 1));
			out.push(format!("DTSTAMP:{}", t::ical_utc(t::now())));
			let new: Vec<&String> = wanted.iter().filter(|w| !seen_attendees.contains(w)).collect();
			if !new.is_empty() && organizer.is_empty() {
				// Becoming a meeting: whoever saves it organises it.
				out.push(format!("ORGANIZER:mailto:{address}"));
				out.push(format!("ATTENDEE;PARTSTAT=ACCEPTED;ROLE=CHAIR:mailto:{address}"));
			}
			for a in new {
				out.push(format!(
					"ATTENDEE;PARTSTAT=NEEDS-ACTION;ROLE=REQ-PARTICIPANT;RSVP=TRUE:mailto:{a}"
				));
			}
			out.push(line);
			in_event = false;
			continue;
		}
		if in_event && depth == 0 {
			match name.as_str() {
				"DESCRIPTION" if keep_description => {}
				"DTSTART" | "DTEND" | "DURATION" | "SUMMARY" | "LOCATION" | "DESCRIPTION" | "DTSTAMP" => continue,
				"SEQUENCE" => {
					sequence = value.trim().parse().unwrap_or(0);
					continue;
				}
				"ORGANIZER" => organizer = mailto(&value),
				"ATTENDEE" => {
					let email = mailto(&value);
					if email != organizer && !wanted.contains(&email) {
						continue;
					}
					seen_attendees.push(email);
				}
				_ => {}
			}
		}
		out.push(line);
	}
	out.iter().map(|l| fold(l)).collect()
}

/// `text` without one occurrence: an EXDATE on the master and any override
/// of that occurrence removed.
pub fn exclude_ics(text: &str, recurrence_id: &str) -> String {
	let target = parse_line(&format!("X{recurrence_id}")).and_then(|p| Time::of(&p));
	let mut out: Vec<String> = Vec::new();
	let mut block: Vec<String> = Vec::new();
	let mut in_event = false;
	let mut depth = 0;
	for line in unfold(text) {
		let p = parse_line(&line);
		let (name, value) = p
			.as_ref()
			.map(|p| (p.name.as_str(), p.value.as_str()))
			.unwrap_or(("", ""));
		if !in_event {
			if name == "BEGIN" && value.eq_ignore_ascii_case("VEVENT") {
				in_event = true;
				depth = 0;
				block = vec![line];
			} else {
				out.push(line);
			}
			continue;
		}
		if name == "BEGIN" {
			depth += 1;
		}
		if name == "END" && depth > 0 {
			depth -= 1;
			block.push(line);
			continue;
		}
		if name == "END" && value.eq_ignore_ascii_case("VEVENT") {
			block.push(line);
			let rid = block
				.iter()
				.filter_map(|l| parse_line(l))
				.find(|p| p.name == "RECURRENCE-ID")
				.and_then(|p| Time::of(&p));
			match (&rid, &target) {
				(Some(r), Some(tgt)) if r.same_instant(tgt) => {}
				(None, _) => {
					let at = block.len() - 1;
					block.insert(at, format!("EXDATE{recurrence_id}"));
					out.append(&mut block);
				}
				_ => out.append(&mut block),
			}
			block.clear();
			in_event = false;
			continue;
		}
		block.push(line);
	}
	out.iter().map(|l| fold(l)).collect()
}

// ── Network ───────────────────────────────────────────────────────────────────

fn check(resp: http::Response) -> Result<http::Response, ApiError> {
	match resp.status {
		200..=299 => Ok(resp),
		401 => Err(ApiError::Auth(
			"The server refused the sign-in: check the account's app password".into(),
		)),
		403 => Err(ApiError::Other("The server does not allow this".into())),
		412 => Err(ApiError::Other(
			"The event changed elsewhere: try again after the next sync".into(),
		)),
		s => Err(ApiError::Other(format!("The calendar server answered {s}"))),
	}
}

async fn propfind(ctx: &Ctx, url: &str, depth: &str, props: &str) -> Result<(Node, String), ApiError> {
	let body = format!(
		r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:prop>{props}</d:prop></d:propfind>"#
	);
	let req = ctx
		.apply(Request::new("PROPFIND", url))
		.header("Depth", depth)
		.body("application/xml; charset=utf-8", body);
	let resp = check(http::send(req).await?)?;
	let node = parse_xml(&resp.body).ok_or_else(|| ApiError::Other("The calendar server sent malformed XML".into()))?;
	Ok((node, url.to_string()))
}

/// `(href, props)` for each `response` of a multistatus whose propstat is 200.
fn responses(doc: &Node) -> Vec<(String, &Node)> {
	let mut rs = Vec::new();
	doc.all("response", &mut rs);
	rs.into_iter()
		.filter_map(|r| {
			let href = r.child("href")?.text.trim().to_string();
			let props = r
				.children
				.iter()
				.filter(|c| c.name == "propstat")
				.find(|ps| ps.child("status").is_none_or(|s| s.text.contains(" 200")))
				.and_then(|ps| ps.child("prop"))?;
			Some((href, props))
		})
		.collect()
}

/// Calendar home URLs found per starting URL and user, so a sync costs one
/// PROPFIND for the list rather than three.
fn homes() -> &'static Mutex<HashMap<String, String>> {
	static HOMES: std::sync::OnceLock<Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
	HOMES.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn discover_home(start: &str, ctx: &Ctx) -> Result<String, ApiError> {
	const PROPS: &str = "<d:current-user-principal/><c:calendar-home-set/><d:resourcetype/>";
	let origin = http::origin(start);
	let mut tried = Vec::new();
	let mut candidates = vec![start.to_string()];
	if matches!(&start[origin.len()..], "" | "/") {
		candidates.insert(0, format!("{origin}/.well-known/caldav"));
		// Servers that keep CalDAV under a path and skip the well-known one.
		candidates.push(format!("{origin}/caldav/"));
	}
	let mut last_err = None;
	for url in candidates {
		let (doc, at) = match propfind(ctx, &url, "0", PROPS).await {
			Ok(r) => r,
			Err(ApiError::Auth(m)) => return Err(ApiError::Auth(m)),
			Err(e) => {
				last_err = Some(e);
				continue;
			}
		};
		tried.push(url.clone());
		if let Some(home) = doc.find("calendar-home-set").and_then(|h| h.text_of("href")) {
			return Ok(http::resolve(&at, &home));
		}
		// The URL may be a calendar itself, or the home set.
		if doc.find("resourcetype").is_some_and(|r| r.child("calendar").is_some()) {
			return Ok(url);
		}
		if let Some(principal) = doc.find("current-user-principal").and_then(|p| p.text_of("href")) {
			let principal = http::resolve(&at, &principal);
			let (doc, at) = propfind(ctx, &principal, "0", "<c:calendar-home-set/>").await?;
			if let Some(home) = doc.find("calendar-home-set").and_then(|h| h.text_of("href")) {
				return Ok(http::resolve(&at, &home));
			}
		}
	}
	Err(last_err.unwrap_or_else(|| ApiError::Other(format!("No CalDAV calendars found at {}", tried.join(", ")))))
}

fn hex_color(c: &str) -> String {
	let c = c.trim();
	if c.starts_with('#') && (c.len() == 9 || c.len() == 7) {
		c[..7].to_string()
	} else {
		"#34aadc".into()
	}
}

pub async fn calendars(account: &str, start: &str, ctx: &Ctx) -> Result<Vec<Calendar>, ApiError> {
	let cache_key = format!(
		"{start}\u{0}{}",
		match ctx {
			Ctx::Basic(u, _) => u.as_str(),
			Ctx::Bearer(_) | Ctx::None => "",
		}
	);
	let cached = homes().lock().unwrap().get(&cache_key).cloned();
	let home = match cached {
		Some(h) => h,
		None => {
			let h = discover_home(start, ctx).await?;
			homes().lock().unwrap().insert(cache_key.clone(), h.clone());
			h
		}
	};
	const PROPS: &str = "<d:displayname/><d:resourcetype/><a:calendar-color/><c:supported-calendar-component-set/><d:current-user-privilege-set/>";
	let (doc, at) = match propfind(ctx, &home, "1", PROPS).await {
		Ok(r) => r,
		Err(e) => {
			homes().lock().unwrap().remove(&cache_key);
			return Err(e);
		}
	};
	let mut out = Vec::new();
	for (href, props) in responses(&doc) {
		if props.find("resourcetype").is_none_or(|r| r.child("calendar").is_none()) {
			continue;
		}
		if let Some(set) = props.find("supported-calendar-component-set") {
			let mut comps = Vec::new();
			set.all("comp", &mut comps);
			if !comps.is_empty()
				&& !comps
					.iter()
					.any(|c| c.attr("name").is_some_and(|n| n.eq_ignore_ascii_case("VEVENT")))
			{
				continue;
			}
		}
		let writable = match props.find("current-user-privilege-set") {
			Some(set) => ["write", "write-content", "all"].iter().any(|p| set.find(p).is_some()),
			None => true,
		};
		let url = http::resolve(&at, &href);
		let url = if url.ends_with('/') { url } else { format!("{url}/") };
		let name = props
			.text_of("displayname")
			.filter(|n| !n.is_empty())
			.unwrap_or_else(|| {
				href.trim_end_matches('/')
					.rsplit('/')
					.next()
					.unwrap_or("Calendar")
					.to_string()
			});
		out.push(Calendar {
			key: format!("{account}/{href}"),
			account: account.to_string(),
			id: href.clone(),
			name,
			color: hex_color(&props.text_of("calendar-color").unwrap_or_default()),
			primary: false,
			writable,
			url,
		});
	}
	if let Some(first) = out.iter_mut().find(|c| c.writable) {
		first.primary = true;
	}
	Ok(out)
}

pub async fn events(address: &str, cal: &Calendar, ctx: &Ctx, from: i64, to: i64) -> Result<Vec<CalEvent>, ApiError> {
	let body = format!(
		r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{}" end="{}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#,
		t::ical_utc(from),
		t::ical_utc(to)
	);
	let req = ctx
		.apply(Request::new("REPORT", &cal.url))
		.header("Depth", "1")
		.body("application/xml; charset=utf-8", body);
	let resp = check(http::send(req).await?)?;
	let doc = parse_xml(&resp.body).ok_or_else(|| ApiError::Other("The calendar server sent malformed XML".into()))?;
	let mut out = Vec::new();
	for (href, props) in responses(&doc) {
		let Some(data) = props.find("calendar-data").map(|d| d.text.clone()) else {
			continue;
		};
		let etag = props.text_of("getetag").unwrap_or_default();
		let href = http::resolve(&cal.url, &href);
		out.extend(occurrences(&data, &href, &etag, cal, address, from, to));
	}
	Ok(out)
}

async fn get(ctx: &Ctx, href: &str) -> Result<(String, String), ApiError> {
	let resp = check(http::send(ctx.apply(Request::new("GET", href))).await?)?;
	let etag = resp.header("ETag").unwrap_or_default().to_string();
	Ok((resp.body, etag))
}

async fn put(ctx: &Ctx, href: &str, ics: String, etag: Option<&str>) -> Result<http::Response, ApiError> {
	let mut req = ctx
		.apply(Request::new("PUT", href))
		.body("text/calendar; charset=utf-8", ics);
	req = match etag {
		Some(e) if !e.is_empty() => req.header("If-Match", e),
		Some(_) => req,
		None => req.header("If-None-Match", "*"),
	};
	Ok(http::send(req).await?)
}

pub async fn save(
	cal: &Calendar,
	ctx: &Ctx,
	draft: &Draft,
	existing: Option<&CalEvent>,
	address: &str,
) -> Result<(), ApiError> {
	match existing {
		None => {
			let uid = format!("{}@octoweb", crate::mail::hex(&crate::workspace::random_bytes16()));
			let href = format!("{}{uid}.ics", cal.url);
			let resp = put(ctx, &href, new_ics(&uid, draft, address, false), None).await?;
			// A server that wants a VTIMEZONE for the zone gets UTC instead.
			if !draft.recur.is_empty() && !draft.all_day && matches!(resp.status, 400 | 403 | 415 | 422) {
				check(put(ctx, &href, new_ics(&uid, draft, address, true), None).await?)?;
			} else {
				check(resp)?;
			}
		}
		Some(e) if e.recurring => {
			return Err(ApiError::Other(
				"Edit repeating events in the calendar's own app for now.".into(),
			));
		}
		Some(e) => {
			let (text, etag) = get(ctx, &e.id).await?;
			check(
				put(
					ctx,
					&e.id,
					edit_ics(
						&text,
						draft,
						address,
						calendar::plain(&draft.description) == e.description,
					),
					Some(&etag),
				)
				.await?,
			)?;
		}
	}
	Ok(())
}

/// Store one calendar object from a file under its UID. `false` when the
/// calendar already holds that UID.
pub async fn import(cal: &Calendar, ctx: &Ctx, uid: &str, object: &str) -> Result<bool, ApiError> {
	let name: String = uid
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() || "-_.@".contains(c) {
				c
			} else {
				'-'
			}
		})
		.take(120)
		.collect();
	let resp = put(ctx, &format!("{}{name}.ics", cal.url), object.to_string(), None).await?;
	if resp.status == 412 {
		return Ok(false);
	}
	check(resp).map(|_| true)
}

/// A start or end in wall-clock form, `2026-10-22T10:00:00`.
pub fn wall(t: &Time) -> String {
	format!(
		"{}T{:02}:{:02}:{:02}",
		t.date.iso(),
		t.secs / 3600,
		t.secs % 3600 / 60,
		t.secs % 60
	)
}

pub async fn delete(ctx: &Ctx, event: &CalEvent, all: bool) -> Result<(), ApiError> {
	if event.recurring && !all && !event.recurrence_id.is_empty() {
		let (text, etag) = get(ctx, &event.id).await?;
		check(put(ctx, &event.id, exclude_ics(&text, &event.recurrence_id), Some(&etag)).await?)?;
		return Ok(());
	}
	let resp = http::send(ctx.apply(Request::new("DELETE", &event.id))).await?;
	if resp.status == 404 {
		return Ok(());
	}
	check(resp).map(|_| ())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn cal() -> Calendar {
		Calendar {
			key: "k/cal/".into(),
			account: "k".into(),
			id: "/cal/".into(),
			name: "Home".into(),
			color: "#000".into(),
			primary: true,
			writable: true,
			url: "https://dav.x/cal/".into(),
		}
	}

	#[test]
	fn xml_parses_namespaces_cdata_and_entities() {
		let doc = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
 <d:response><d:href>/cal/a%20b.ics</d:href>
  <d:propstat><d:prop><d:getetag>"e1"</d:getetag><cal:calendar-data><![CDATA[BEGIN:VCALENDAR
X:1&2]]></cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><d:displayname/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
 </d:response>
 <d:response><d:href>/cal/</d:href><d:propstat><d:prop><d:displayname>Work &amp; life</d:displayname><x:comp name='VEVENT' xmlns:x="y"/></d:prop></d:propstat></d:response>
</d:multistatus>"#;
		let root = parse_xml(doc).unwrap();
		assert_eq!(root.name, "multistatus");
		let rs = responses(&root);
		assert_eq!(rs.len(), 2);
		assert_eq!(rs[0].0, "/cal/a%20b.ics");
		assert_eq!(rs[0].1.text_of("getetag").unwrap(), "\"e1\"");
		assert_eq!(rs[0].1.find("calendar-data").unwrap().text, "BEGIN:VCALENDAR\nX:1&2");
		assert_eq!(rs[1].1.text_of("displayname").unwrap(), "Work & life");
		assert_eq!(rs[1].1.find("comp").unwrap().attr("name"), Some("VEVENT"));
	}

	const SERIES: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:DAYLIGHT\r\nDTSTART:19700329T020000\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:s1\r\nSUMMARY:Weekly\\, sync\r\nDTSTART;TZID=Europe/Berlin:20261019T100000\r\nDTEND;TZID=Europe/Berlin:20261019T103000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6\r\nEXDATE;TZID=Europe/Berlin:20261021T100000\r\nORGANIZER;CN=Boss:mailto:boss@x.com\r\n\
ATTENDEE;CN=Me;PARTSTAT=TENTATIVE:mailto:me@x.com\r\nATTENDEE;CUTYPE=ROOM:mailto:room@x.com\r\nLOCATION:https://zoom.us/j/1\r\n\
\u{20}23\r\nBEGIN:VALARM\r\nTRIGGER:-PT5M\r\nDESCRIPTION:ignore me\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s1\r\nRECURRENCE-ID;TZID=Europe/Berlin:20261026T100000\r\nSUMMARY:Moved\r\nDTSTART;TZID=Europe/Berlin:20261026T150000\r\nDTEND;TZID=Europe/Berlin:20261026T153000\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

	#[test]
	fn series_expand_with_exdates_overrides_and_zones() {
		let from = Date::new(2026, 10, 1).days() * 86_400;
		let to = Date::new(2026, 12, 1).days() * 86_400;
		let occ = occurrences(
			SERIES,
			"https://dav.x/cal/s1.ics",
			"\"e\"",
			&cal(),
			"me@x.com",
			from,
			to,
		);
		let starts: Vec<(String, String)> = occ.iter().map(|e| (t::rfc3339_utc(e.start), e.title.clone())).collect();
		assert_eq!(
			starts,
			[
				("2026-10-19T08:00:00Z".to_string(), "Weekly, sync".to_string()),
				// 21 Oct is an EXDATE; 26 Oct moved to 15:00, now CET (UTC+1).
				("2026-10-28T09:00:00Z".into(), "Weekly, sync".into()),
				("2026-11-02T09:00:00Z".into(), "Weekly, sync".into()),
				("2026-11-04T09:00:00Z".into(), "Weekly, sync".into()),
				("2026-10-26T14:00:00Z".into(), "Moved".into()),
			]
		);
		let first = &occ[0];
		assert_eq!(first.end - first.start, 1800);
		assert_eq!(first.me, "tentative");
		assert_eq!(first.join, "https://zoom.us/j/123");
		assert_eq!(first.attendees.len(), 1, "rooms are not guests");
		assert!(first.recurring && !first.editable);
		assert_eq!(first.recurrence_id, ";TZID=Europe/Berlin:20261019T100000");
		assert!(first.key.ends_with("s1.ics#;TZID=Europe/Berlin:20261019T100000"));
	}

	#[test]
	fn rules_cover_monthly_yearly_and_until() {
		let r = parse_rule("FREQ=MONTHLY;BYDAY=-1FR").unwrap();
		assert_eq!(
			expand_dates(Date::new(2026, 10, 1), &r, Date::new(2026, 12, 31)),
			[
				Date::new(2026, 10, 30),
				Date::new(2026, 11, 27),
				Date::new(2026, 12, 25)
			]
		);
		let r = parse_rule("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=2").unwrap();
		assert_eq!(
			expand_dates(Date::new(2026, 10, 1), &r, Date::new(2027, 12, 31)),
			[Date::new(2026, 10, 30), Date::new(2026, 11, 30)]
		);
		let r = parse_rule("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH").unwrap();
		assert_eq!(
			expand_dates(Date::new(2025, 11, 27), &r, Date::new(2026, 12, 1)),
			[Date::new(2025, 11, 27), Date::new(2026, 11, 26)]
		);
		let r = parse_rule("FREQ=MONTHLY;BYMONTHDAY=31").unwrap();
		assert_eq!(
			expand_dates(Date::new(2026, 10, 31), &r, Date::new(2027, 1, 31)),
			[Date::new(2026, 10, 31), Date::new(2026, 12, 31), Date::new(2027, 1, 31)]
		);
		let r = parse_rule("FREQ=DAILY;INTERVAL=2").unwrap();
		assert_eq!(
			expand_dates(Date::new(2026, 10, 30), &r, Date::new(2026, 11, 4)).len(),
			3
		);
		let r = parse_rule("FREQ=WEEKLY;INTERVAL=2").unwrap();
		assert_eq!(
			expand_dates(Date::new(2026, 10, 22), &r, Date::new(2026, 11, 20)),
			[Date::new(2026, 10, 22), Date::new(2026, 11, 5), Date::new(2026, 11, 19)]
		);

		let ics = "BEGIN:VEVENT\nUID:u\nDTSTART:20261020T080000Z\nDURATION:PT1H\nRRULE:FREQ=DAILY;UNTIL=20261022T080000Z\nEND:VEVENT\n";
		let occ = occurrences(ics, "h", "", &cal(), "", 0, Date::new(2030, 1, 1).days() * 86_400);
		assert_eq!(occ.len(), 3);
		assert_eq!(occ[2].end - occ[2].start, 3600);
	}

	#[test]
	fn all_day_events_keep_their_dates() {
		let ics = "BEGIN:VEVENT\nUID:d\nDTSTART;VALUE=DATE:20261022\nDTEND;VALUE=DATE:20261024\nSUMMARY:Trip\nTRANSP:TRANSPARENT\nEND:VEVENT";
		let occ = occurrences(ics, "h", "", &cal(), "", 0, Date::new(2030, 1, 1).days() * 86_400);
		assert_eq!(occ.len(), 1);
		assert!(occ[0].all_day && !occ[0].busy && occ[0].editable);
		assert_eq!(
			(occ[0].start_date.as_str(), occ[0].end_date.as_str()),
			("2026-10-22", "2026-10-24")
		);
		assert_eq!(occ[0].start, t::local_midnight(Date::new(2026, 10, 22)));
	}

	#[test]
	fn writing_round_trips_and_edits_in_place() {
		let draft = Draft {
			title: "Lunch; with, Anna".into(),
			start: 1_792_656_000,
			end: 1_792_659_600,
			location: "Café".into(),
			description: "line1\nline2".into(),
			attendees: vec!["anna@x.com".into()],
			..Default::default()
		};
		let ics = new_ics("u1", &draft, "me@x.com", false);
		assert!(ics.contains("SUMMARY:Lunch\\; with\\, Anna\r\n"));
		assert!(ics.contains("DTSTART:20261022T080000Z\r\n"));
		let back = parse_ics(&ics, "me@x.com");
		assert_eq!(back[0].summary, "Lunch; with, Anna");
		assert_eq!(back[0].description, "line1\nline2");
		assert_eq!(back[0].attendees.len(), 2);

		let mut edit = draft.clone();
		edit.title = "Lunch".into();
		edit.attendees = vec![];
		edit.start += 3600;
		edit.end += 3600;
		let original = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:u1\r\nX-KEEP:yes\r\nSEQUENCE:2\r\nSUMMARY:old\r\nDTSTART:20261022T080000Z\r\nDURATION:PT1H\r\nORGANIZER:mailto:me@x.com\r\nATTENDEE:mailto:me@x.com\r\nATTENDEE:mailto:anna@x.com\r\nBEGIN:VALARM\r\nDESCRIPTION:a\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
		let edited = edit_ics(original, &edit, "me@x.com", false);
		assert!(
			edited.contains("X-KEEP:yes\r\n") && edited.contains("SEQUENCE:3\r\n") && edited.contains("BEGIN:VALARM")
		);
		assert!(!edited.contains("anna@x.com") && !edited.contains("DURATION"));
		let e = &parse_ics(&edited, "me@x.com")[0];
		assert_eq!(e.summary, "Lunch");
		assert_eq!(e.start.as_ref().unwrap().ts(), 1_792_659_600);

		let excluded = exclude_ics(SERIES, ";TZID=Europe/Berlin:20261026T100000");
		assert!(excluded.contains("EXDATE;TZID=Europe/Berlin:20261026T100000\r\n"));
		assert!(!excluded.contains("SUMMARY:Moved"));
		assert!(excluded.contains("BEGIN:VTIMEZONE"));
	}

	/// Against a live CalDAV server holding one calendar for `me@x.com` /
	/// `secret`, e.g. Radicale on 127.0.0.1:5232:
	/// `OCTOWEB_DAV_TEST=http://127.0.0.1:5232/ cargo test --bin octoweb dav_round_trip -- --ignored`
	#[tokio::test]
	#[ignore]
	async fn dav_round_trip_against_a_server() {
		let start = std::env::var("OCTOWEB_DAV_TEST").expect("OCTOWEB_DAV_TEST names the server");
		let ctx = Ctx::Basic("me@x.com".into(), "secret".into());
		let cals = calendars("acct", &start, &ctx).await.expect("discovery");
		let cal = cals.iter().find(|c| c.writable).expect("a writable calendar").clone();
		let today = t::local_date(t::now());
		let (from, to) = (
			t::local_midnight(today.add_days(-1)),
			t::local_midnight(today.add_days(30)),
		);
		let at = |days: i64, h: u32| t::local_ts(today.add_days(days), h * 60);

		let one = Draft {
			title: "E2E single".into(),
			start: at(1, 10),
			end: at(1, 11),
			location: "Room, 1".into(),
			..Default::default()
		};
		save(&cal, &ctx, &one, None, "me@x.com").await.expect("create");
		let weekly = Draft {
			title: "E2E weekly".into(),
			start: at(2, 9),
			end: at(2, 9) + 1800,
			recur: "weekly".into(),
			..Default::default()
		};
		save(&cal, &ctx, &weekly, None, "me@x.com")
			.await
			.expect("create series");

		let list = events("me@x.com", &cal, &ctx, from, to).await.expect("query");
		let single = list
			.iter()
			.find(|e| e.title == "E2E single")
			.expect("single is listed")
			.clone();
		assert_eq!((single.start, single.location.as_str()), (at(1, 10), "Room, 1"));
		let series: Vec<&CalEvent> = list.iter().filter(|e| e.title == "E2E weekly").collect();
		assert!(series.len() >= 4, "weekly occurrences in a month: {}", series.len());
		assert!(series.iter().all(|e| e.recurring && e.end - e.start == 1800));

		// Edit the single event; drop the second occurrence of the series.
		let mut edit = one.clone();
		edit.title = "E2E edited".into();
		edit.key = single.key.clone();
		save(&cal, &ctx, &edit, Some(&single), "me@x.com").await.expect("edit");
		let second = series[1].clone();
		delete(&ctx, &second, false).await.expect("drop one occurrence");

		let list = events("me@x.com", &cal, &ctx, from, to).await.expect("query again");
		assert!(list.iter().any(|e| e.title == "E2E edited") && !list.iter().any(|e| e.title == "E2E single"));
		let left: Vec<i64> = list
			.iter()
			.filter(|e| e.title == "E2E weekly")
			.map(|e| e.start)
			.collect();
		assert_eq!(left.len(), series.len() - 1);
		assert!(!left.contains(&second.start));

		// Import a file: new UIDs are added, a second import adds nothing.
		for (uid, object) in split_objects(&FEED.replace("2026", &today.y.to_string())) {
			import(&cal, &ctx, &uid, &object).await.expect("import");
			assert!(
				!import(&cal, &ctx, &uid, &object).await.expect("import again"),
				"already there"
			);
		}
		let imported = events("me@x.com", &cal, &ctx, from, t::local_midnight(today.add_days(400)))
			.await
			.expect("query imported");
		assert!(imported.iter().any(|e| e.title == "Christmas" && e.uid == "a"));
		for e in imported.iter().filter(|e| e.uid == "a" || e.uid == "b") {
			delete(&ctx, e, true).await.expect("delete imported");
		}
		// The collection itself, read as a feed.
		let feed = http::send(ctx.apply(Request::new("GET", &cal.url)))
			.await
			.expect("GET collection");
		assert!(feed.ok() && feed.body.contains("BEGIN:VCALENDAR"));

		// Clean up.
		let edited = list.iter().find(|e| e.title == "E2E edited").unwrap().clone();
		delete(&ctx, &edited, false).await.expect("delete single");
		delete(&ctx, list.iter().find(|e| e.title == "E2E weekly").unwrap(), true)
			.await
			.expect("delete series");
		let list = events("me@x.com", &cal, &ctx, from, to).await.expect("final query");
		assert!(!list.iter().any(|e| e.title.starts_with("E2E")));
	}

	const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Gov//Holidays//EN\r\nMETHOD:PUBLISH\r\nX-WR-CALNAME:Public holidays\r\n\
BEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\n\
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:19700329T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\n\
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:a\r\nDTSTART;VALUE=DATE:20261225\r\nDTEND;VALUE=DATE:20261226\r\nSUMMARY:Christmas\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nDTSTART;TZID=Europe/Berlin:20261019T100000\r\nDURATION:PT1H\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nSUMMARY:Weekly\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nRECURRENCE-ID;TZID=Europe/Berlin:20261026T100000\r\nDTSTART;TZID=Europe/Berlin:20261026T120000\r\nDURATION:PT1H\r\nSUMMARY:Weekly (moved)\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

	#[test]
	fn files_split_per_uid_with_their_zones() {
		let objects = split_objects(FEED);
		assert_eq!(objects.iter().map(|(u, _)| u.as_str()).collect::<Vec<_>>(), ["a", "b"]);
		let b = &objects[1].1;
		assert!(
			b.contains("BEGIN:VTIMEZONE")
				&& b.contains("TZOFFSETTO:+0200")
				&& b.contains("Weekly (moved)")
				&& !b.contains("METHOD")
		);
		assert!(!b.contains("Christmas"));
		assert!(!objects[0].1.contains("VTIMEZONE"), "only the zones an event uses");
		assert_eq!(parse_ics(b, "").len(), 2);
		assert_eq!(calendar_name(FEED).as_deref(), Some("Public holidays"));
		assert_eq!(method("BEGIN:VCALENDAR\nMETHOD:request\nEND:VCALENDAR"), "REQUEST");
	}

	#[test]
	fn feeds_expand_into_read_only_occurrences() {
		let mut feed = cal();
		feed.writable = false;
		let from = Date::new(2026, 10, 1).days() * 86_400;
		let to = Date::new(2027, 1, 1).days() * 86_400;
		let occ = feed_occurrences(FEED, "https://x/h.ics", &feed, from, to);
		let titles: Vec<&str> = occ.iter().map(|e| e.title.as_str()).collect();
		assert_eq!(titles, ["Christmas", "Weekly", "Weekly", "Weekly (moved)"]);
		assert!(occ.iter().all(|e| !e.editable));
		assert_eq!(occ[0].uid, "a");
		assert!(occ[0].key.contains("https://x/h.ics#a"));
	}

	#[test]
	fn long_lines_fold_on_character_boundaries() {
		let line = format!("DESCRIPTION:{}", "é".repeat(60));
		let folded = fold(&line);
		assert!(folded.split("\r\n").all(|l| l.len() <= 75));
		assert_eq!(unfold(&folded)[0], line);
	}
}
