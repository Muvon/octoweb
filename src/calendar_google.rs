//! Google Calendar API v3 for Gmail and Google Workspace accounts. Recurring
//! events come expanded (`singleEvents=true`), so each occurrence has its own
//! id; edits and answers apply to that occurrence.

use serde_json::{json, Value};

use crate::calendar::{self, ApiError, Attendee, CalEvent, Calendar, Ctx, Draft};
use crate::calendar_time::{self as t, Date};
use crate::http::{self, Request};

const API: &str = "https://www.googleapis.com/calendar/v3";

/// The JSON of a successful response, or why it failed. A missing or revoked
/// grant asks for a new sign-in; a disabled API says where to turn it on.
pub(crate) fn check(resp: http::Response) -> Result<Value, ApiError> {
	if resp.ok() {
		return Ok(if resp.body.trim().is_empty() {
			Value::Null
		} else {
			resp.json()?
		});
	}
	let body: Value = serde_json::from_str(&resp.body).unwrap_or(Value::Null);
	let message = body["error"]["message"].as_str().unwrap_or("").to_string();
	let reasons: Vec<&str> = body["error"]["errors"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|e| e["reason"].as_str())
		.chain(
			body["error"]["details"]
				.as_array()
				.into_iter()
				.flatten()
				.filter_map(|d| d["reason"].as_str()),
		)
		.collect();
	if resp.status == 401 {
		return Err(ApiError::Auth(
			"Google sign-in expired: connect the calendar again".into(),
		));
	}
	if reasons
		.iter()
		.any(|r| matches!(*r, "accessNotConfigured" | "SERVICE_DISABLED"))
	{
		return Err(ApiError::Other(format!(
			"The Google Calendar API is off for the OAuth client's Cloud project. Turn it on: https://console.cloud.google.com/apis/library/calendar-json.googleapis.com — {message}"
		)));
	}
	if resp.status == 403
		&& reasons
			.iter()
			.any(|r| matches!(*r, "insufficientPermissions" | "ACCESS_TOKEN_SCOPE_INSUFFICIENT"))
	{
		return Err(ApiError::Auth(
			"Calendar access was not granted: connect the calendar again".into(),
		));
	}
	Err(ApiError::Other(if message.is_empty() {
		format!("Google Calendar answered {}", resp.status)
	} else {
		message
	}))
}

pub async fn calendars(account: &str, ctx: &Ctx) -> Result<Vec<Calendar>, ApiError> {
	let mut out = Vec::new();
	let mut page = String::new();
	loop {
		let mut url = format!("{API}/users/me/calendarList?maxResults=250");
		if !page.is_empty() {
			url.push_str(&format!("&pageToken={}", http::encode(&page)));
		}
		let body = check(http::send(ctx.apply(Request::new("GET", &url))).await?)?;
		for c in body["items"].as_array().into_iter().flatten() {
			let role = c["accessRole"].as_str().unwrap_or("");
			if c["hidden"].as_bool() == Some(true) || role == "freeBusyReader" || c["deleted"].as_bool() == Some(true) {
				continue;
			}
			let id = c["id"].as_str().unwrap_or_default().to_string();
			let name = c["summaryOverride"]
				.as_str()
				.or(c["summary"].as_str())
				.unwrap_or(&id)
				.to_string();
			out.push(Calendar {
				key: format!("{account}/{id}"),
				account: account.to_string(),
				name,
				color: c["backgroundColor"].as_str().unwrap_or("#4285f4").to_string(),
				primary: c["primary"].as_bool() == Some(true),
				writable: matches!(role, "owner" | "writer"),
				url: String::new(),
				id,
			});
		}
		match body["nextPageToken"].as_str() {
			Some(next) => page = next.to_string(),
			None => return Ok(out),
		}
	}
}

/// A start or end: `(instant, date)`, the date set for all-day events.
fn moment(v: &Value) -> Option<(i64, Option<Date>)> {
	if let Some(d) = v["date"].as_str() {
		let date = Date::parse(d)?;
		return Some((t::local_midnight(date), Some(date)));
	}
	let ts = t::parse_rfc3339(v["dateTime"].as_str()?, v["timeZone"].as_str().unwrap_or(""))?;
	Some((ts, None))
}

/// One API event as a store occurrence; `None` for cancelled occurrences and
/// working-location markers.
pub fn occurrence(e: &Value, cal: &Calendar, address: &str) -> Option<CalEvent> {
	if e["status"].as_str() == Some("cancelled") {
		return None;
	}
	let kind = match e["eventType"].as_str().unwrap_or("default") {
		"workingLocation" => return None,
		"outOfOffice" => "ooo",
		"focusTime" => "focus",
		"birthday" => "birthday",
		_ => "",
	};
	let id = e["id"].as_str()?.to_string();
	let (start, start_date) = moment(&e["start"])?;
	let (end, end_date) = moment(&e["end"])?;
	let attendees: Vec<Attendee> = e["attendees"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|a| a["resource"].as_bool() != Some(true))
		.map(|a| {
			let email = a["email"].as_str().unwrap_or_default().to_ascii_lowercase();
			Attendee {
				me: a["self"].as_bool() == Some(true) || email == address,
				name: a["displayName"].as_str().unwrap_or_default().to_string(),
				status: calendar::status_word(a["responseStatus"].as_str().unwrap_or("")),
				organizer: a["organizer"].as_bool() == Some(true),
				optional: a["optional"].as_bool() == Some(true),
				email,
			}
		})
		.collect();
	let me = attendees
		.iter()
		.find(|a| a.me)
		.map(|a| a.status.clone())
		.unwrap_or_default();
	let organizer = e["organizer"]["email"]
		.as_str()
		.unwrap_or_default()
		.to_ascii_lowercase();
	let mine = e["organizer"]["self"].as_bool() == Some(true) || organizer.is_empty() || organizer == address;
	let video: Vec<&str> = e["conferenceData"]["entryPoints"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|p| p["entryPointType"].as_str() == Some("video"))
		.filter_map(|p| p["uri"].as_str())
		.collect();
	let conference = format!("{} {}", e["hangoutLink"].as_str().unwrap_or_default(), video.join(" "));
	let location = e["location"].as_str().unwrap_or_default();
	let raw_description = e["description"].as_str().unwrap_or_default();
	let title = match e["summary"].as_str() {
		Some(s) if !s.trim().is_empty() => s.trim().to_string(),
		_ if kind == "ooo" => "Out of office".into(),
		_ => "(No title)".into(),
	};
	Some(CalEvent {
		key: format!("{}/{id}", cal.key),
		account: cal.account.clone(),
		calendar: cal.key.clone(),
		series: e["recurringEventId"].as_str().unwrap_or_default().to_string(),
		title,
		start,
		end,
		all_day: start_date.is_some(),
		start_date: start_date.map(Date::iso).unwrap_or_default(),
		end_date: end_date.map(Date::iso).unwrap_or_default(),
		location: location.to_string(),
		description: calendar::plain(raw_description),
		link: e["htmlLink"].as_str().unwrap_or_default().to_string(),
		join: calendar::find_join(&[&conference, location, raw_description]),
		organizer,
		me,
		busy: e["transparency"].as_str() != Some("transparent"),
		recurring: e["recurringEventId"].is_string(),
		editable: cal.writable && mine,
		kind: kind.to_string(),
		attendees,
		etag: String::new(),
		recurrence_id: String::new(),
		uid: e["iCalUID"].as_str().unwrap_or_default().to_string(),
		id,
	})
}

pub async fn events(address: &str, cal: &Calendar, ctx: &Ctx, from: i64, to: i64) -> Result<Vec<CalEvent>, ApiError> {
	let mut out = Vec::new();
	let mut page = String::new();
	loop {
		let mut url = format!(
			"{API}/calendars/{}/events?singleEvents=true&maxResults=2500&timeMin={}&timeMax={}",
			http::encode(&cal.id),
			http::encode(&t::rfc3339_utc(from)),
			http::encode(&t::rfc3339_utc(to)),
		);
		if !page.is_empty() {
			url.push_str(&format!("&pageToken={}", http::encode(&page)));
		}
		let body = check(http::send(ctx.apply(Request::new("GET", &url))).await?)?;
		out.extend(
			body["items"]
				.as_array()
				.into_iter()
				.flatten()
				.filter_map(|e| occurrence(e, cal, address)),
		);
		match body["nextPageToken"].as_str() {
			Some(next) => page = next.to_string(),
			None => return Ok(out),
		}
	}
}

fn event_url(cal: &Calendar, id: &str) -> String {
	format!("{API}/calendars/{}/events/{}", http::encode(&cal.id), http::encode(id))
}

/// The request body for a draft. `current` is the event as Google has it,
/// for an edit: guests already invited keep their answers.
pub fn body(draft: &Draft, current: Option<&Value>) -> Value {
	let zone = t::local_zone();
	let when = |ts: i64, date: &str| {
		if draft.all_day {
			json!({ "date": date })
		} else {
			json!({ "dateTime": t::rfc3339_utc(ts), "timeZone": zone })
		}
	};
	let mut body = json!({
		"summary": draft.title.trim(),
		"location": draft.location.trim(),
		"description": draft.description,
		"start": when(draft.start, &draft.start_date),
		"end": when(draft.end, &draft.end_date),
	});
	// The pane edits the description as plain text: an unchanged one keeps
	// its formatting and links by not being sent.
	if let Some(c) = current {
		if calendar::plain(c["description"].as_str().unwrap_or_default()) == calendar::plain(&draft.description) {
			body.as_object_mut().expect("object").remove("description");
		}
	}
	let existing: Vec<Value> = current
		.and_then(|c| c["attendees"].as_array().cloned())
		.unwrap_or_default();
	let wanted: Vec<String> = draft.attendees.iter().map(|a| a.trim().to_ascii_lowercase()).collect();
	let mut attendees: Vec<Value> = existing
		.into_iter()
		.filter(|a| {
			let email = a["email"].as_str().unwrap_or_default().to_ascii_lowercase();
			a["self"].as_bool() == Some(true)
				|| a["organizer"].as_bool() == Some(true)
				|| a["resource"].as_bool() == Some(true)
				|| wanted.contains(&email)
		})
		.collect();
	for email in &wanted {
		if !attendees
			.iter()
			.any(|a| a["email"].as_str().map(str::to_ascii_lowercase).as_deref() == Some(email))
		{
			attendees.push(json!({ "email": email }));
		}
	}
	if !attendees.is_empty() || current.is_some_and(|c| c["attendees"].is_array()) {
		body["attendees"] = Value::Array(attendees);
	}
	if current.is_none() {
		if let Some(rule) = calendar::rrule(&draft.recur) {
			body["recurrence"] = json!([format!("RRULE:{rule}")]);
		}
	}
	let has_video = current.is_some_and(|c| c["conferenceData"].is_object() || c["hangoutLink"].is_string());
	if draft.video && !has_video {
		body["conferenceData"] = json!({
			"createRequest": {
				"requestId": crate::mail::hex(&crate::workspace::random_bytes16()),
				"conferenceSolutionKey": { "type": "hangoutsMeet" },
			}
		});
	}
	body
}

pub async fn save(cal: &Calendar, ctx: &Ctx, draft: &Draft, existing: Option<&CalEvent>) -> Result<(), ApiError> {
	match existing {
		None => {
			let notify = if draft.attendees.is_empty() { "none" } else { "all" };
			let url = format!(
				"{API}/calendars/{}/events?conferenceDataVersion=1&sendUpdates={notify}",
				http::encode(&cal.id)
			);
			check(http::send(ctx.apply(Request::new("POST", &url)).json(&body(draft, None))).await?)?;
		}
		Some(e) => {
			let current = check(http::send(ctx.apply(Request::new("GET", &event_url(cal, &e.id)))).await?)?;
			let url = format!("{}?conferenceDataVersion=1&sendUpdates=all", event_url(cal, &e.id));
			check(
				http::send(
					ctx.apply(Request::new("PATCH", &url))
						.json(&body(draft, Some(&current))),
				)
				.await?,
			)?;
		}
	}
	Ok(())
}

pub async fn delete(cal: &Calendar, ctx: &Ctx, event: &CalEvent, all: bool) -> Result<(), ApiError> {
	let id = if all && !event.series.is_empty() {
		&event.series
	} else {
		&event.id
	};
	let url = format!("{}?sendUpdates=all", event_url(cal, id));
	let resp = http::send(ctx.apply(Request::new("DELETE", &url))).await?;
	// Already gone is what was asked for.
	if matches!(resp.status, 404 | 410) {
		return Ok(());
	}
	check(resp).map(|_| ())
}

pub async fn rsvp(cal: &Calendar, ctx: &Ctx, event: &CalEvent, response: &str) -> Result<(), ApiError> {
	let current = check(http::send(ctx.apply(Request::new("GET", &event_url(cal, &event.id)))).await?)?;
	let mut attendees = current["attendees"].as_array().cloned().unwrap_or_default();
	let me = attendees
		.iter_mut()
		.find(|a| a["self"].as_bool() == Some(true))
		.ok_or_else(|| ApiError::Other("You are not a guest of this event.".into()))?;
	me["responseStatus"] = json!(response);
	let url = format!("{}?sendUpdates=all", event_url(cal, &event.id));
	check(
		http::send(
			ctx.apply(Request::new("PATCH", &url))
				.json(&json!({ "attendees": attendees })),
		)
		.await?,
	)?;
	Ok(())
}

/// A start or end of an imported event, keeping its zone.
fn import_time(t: &crate::calendar_dav::Time) -> Value {
	use crate::calendar_dav::{wall, TimeKind};
	match &t.kind {
		TimeKind::Date => json!({ "date": t.date.iso() }),
		TimeKind::Utc => json!({ "dateTime": t::rfc3339_utc(t.ts()) }),
		TimeKind::Zone(z) => match t::zone_name(z) {
			Some(name) => json!({ "dateTime": wall(t), "timeZone": name }),
			None => json!({ "dateTime": t::rfc3339_utc(t.ts()), "timeZone": t::local_zone() }),
		},
		TimeKind::Floating => json!({ "dateTime": wall(t), "timeZone": t::local_zone() }),
	}
}

/// The import body for one calendar object (a series with its overrides is
/// imported as the series); `None` when there is nothing to add.
pub fn import_body(object: &str) -> Option<Value> {
	use crate::calendar_dav::TimeKind;
	let events = crate::calendar_dav::parse_ics(object, "");
	let ev = events.iter().find(|e| e.recurrence_id.is_none() && !e.cancelled)?;
	let start = ev.start.as_ref()?;
	let mut start_json = import_time(start);
	if start.kind == TimeKind::Utc && ev.rrule.is_some() {
		// A series needs a zone to repeat in.
		start_json["timeZone"] = json!(t::local_zone());
	}
	let end_json = match (&ev.end, ev.duration) {
		(Some(end), _) => import_time(end),
		(None, d) if start.kind == TimeKind::Date => {
			json!({ "date": start.date.add_days(d.map_or(1, |d| (d / 86_400).max(1))).iso() })
		}
		(None, d) => {
			let mut j = json!({ "dateTime": t::rfc3339_utc(start.ts() + d.unwrap_or(3600)) });
			if let Some(z) = start_json.get("timeZone") {
				j["timeZone"] = z.clone();
			}
			j
		}
	};
	let mut body = json!({
		"iCalUID": ev.uid,
		"summary": ev.summary,
		"description": ev.description,
		"location": ev.location,
		"start": start_json,
		"end": end_json,
	});
	if ev.transparent {
		body["transparency"] = json!("transparent");
	}
	if let Some(rule) = &ev.rrule {
		let mut lines = vec![format!("RRULE:{rule}")];
		lines.extend(ev.exdates.iter().map(|x| format!("EXDATE{}", x.ical())));
		lines.extend(ev.rdates.iter().map(|x| format!("RDATE{}", x.ical())));
		body["recurrence"] = json!(lines);
	}
	if !ev.organizer.is_empty() {
		body["organizer"] = json!({ "email": ev.organizer });
	}
	if !ev.attendees.is_empty() {
		body["attendees"] = Value::Array(
			ev.attendees
				.iter()
				.map(|a| {
					let status = match a.status.as_str() {
						"accepted" => "accepted",
						"declined" => "declined",
						"tentative" => "tentative",
						_ => "needsAction",
					};
					json!({ "email": a.email, "displayName": a.name, "responseStatus": status })
				})
				.collect(),
		);
	}
	Some(body)
}

/// Add one calendar object as a private copy (`events.import`): nobody is
/// invited or told. Importing the same UID again updates that copy.
pub async fn import(cal: &Calendar, ctx: &Ctx, object: &str) -> Result<bool, ApiError> {
	let Some(body) = import_body(object) else {
		return Ok(false);
	};
	let url = format!("{API}/calendars/{}/events/import", http::encode(&cal.id));
	check(http::send(ctx.apply(Request::new("POST", &url)).json(&body)).await?)?;
	Ok(true)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn cal() -> Calendar {
		Calendar {
			key: "k/me@x.com".into(),
			account: "k".into(),
			id: "me@x.com".into(),
			name: "Me".into(),
			color: "#000".into(),
			primary: true,
			writable: true,
			url: String::new(),
		}
	}

	#[test]
	fn api_events_become_occurrences() {
		let e = json!({
			"id": "abc_20261022T080000Z", "status": "confirmed", "summary": "Standup",
			"htmlLink": "https://www.google.com/calendar/event?eid=1",
			"start": { "dateTime": "2026-10-22T10:00:00+02:00", "timeZone": "Europe/Berlin" },
			"end": { "dateTime": "2026-10-22T10:15:00+02:00" },
			"recurringEventId": "abc",
			"organizer": { "email": "boss@x.com" },
			"hangoutLink": "https://meet.google.com/abc-defg-hij",
			"description": "<b>Agenda</b><br>https://zoom.us/j/9",
			"attendees": [
				{ "email": "Me@x.com", "self": true, "responseStatus": "needsAction" },
				{ "email": "boss@x.com", "organizer": true, "responseStatus": "accepted", "displayName": "Boss" },
				{ "email": "room@resource.calendar.google.com", "resource": true }
			]
		});
		let o = occurrence(&e, &cal(), "me@x.com").unwrap();
		assert_eq!(o.key, "k/me@x.com/abc_20261022T080000Z");
		assert_eq!((o.start, o.end), (1_792_656_000, 1_792_656_900));
		assert_eq!(o.join, "https://meet.google.com/abc-defg-hij");
		assert_eq!(o.me, "needs");
		assert_eq!(o.attendees.len(), 2);
		assert!(o.recurring && !o.editable, "someone else's meeting");
		assert_eq!(o.description, "Agenda\nhttps://zoom.us/j/9");

		let all_day = json!({ "id": "d", "start": { "date": "2026-10-22" }, "end": { "date": "2026-10-24" }, "transparency": "transparent" });
		let o = occurrence(&all_day, &cal(), "me@x.com").unwrap();
		assert!(o.all_day && !o.busy && o.editable);
		assert_eq!(
			(o.start_date.as_str(), o.end_date.as_str()),
			("2026-10-22", "2026-10-24")
		);
		assert_eq!(o.title, "(No title)");

		assert!(occurrence(&json!({ "id": "c", "status": "cancelled" }), &cal(), "").is_none());
		assert!(occurrence(&json!({ "id": "w", "eventType": "workingLocation", "start": { "date": "2026-10-22" }, "end": { "date": "2026-10-23" } }), &cal(), "").is_none());
	}

	#[test]
	fn drafts_keep_existing_guests_answers_and_ask_for_meet() {
		let draft = Draft {
			title: " Review ".into(),
			start: 1_792_656_000,
			end: 1_792_659_600,
			attendees: vec!["Anna@x.com".into(), "new@x.com".into()],
			video: true,
			recur: "weekly".into(),
			..Default::default()
		};
		let created = body(&draft, None);
		assert_eq!(created["summary"], "Review");
		assert_eq!(created["start"]["dateTime"], "2026-10-22T08:00:00Z");
		assert_eq!(created["recurrence"][0], "RRULE:FREQ=WEEKLY");
		assert_eq!(
			created["conferenceData"]["createRequest"]["conferenceSolutionKey"]["type"],
			"hangoutsMeet"
		);
		assert_eq!(created["attendees"].as_array().unwrap().len(), 2);

		let current = json!({
			"description": "<b>Notes</b>",
			"hangoutLink": "https://meet.google.com/a",
			"attendees": [
				{ "email": "me@x.com", "self": true, "organizer": true, "responseStatus": "accepted" },
				{ "email": "anna@x.com", "responseStatus": "accepted" },
				{ "email": "gone@x.com", "responseStatus": "declined" }
			]
		});
		let mut draft = draft;
		draft.description = "Notes".into();
		let edited = body(&draft, Some(&current));
		assert!(edited.get("description").is_none(), "unchanged notes keep their markup");
		let emails: Vec<&str> = edited["attendees"]
			.as_array()
			.unwrap()
			.iter()
			.map(|a| a["email"].as_str().unwrap())
			.collect();
		assert_eq!(emails, ["me@x.com", "anna@x.com", "new@x.com"]);
		assert_eq!(edited["attendees"][1]["responseStatus"], "accepted");
		assert!(edited.get("recurrence").is_none() && edited.get("conferenceData").is_none());
	}

	#[test]
	fn imports_keep_zones_repeats_and_guests() {
		let object = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x@y\r\nSUMMARY:Sync\r\nDTSTART;TZID=/mozilla.org/20050126_1/Europe/Berlin:20261019T100000\r\n\
DURATION:PT30M\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEXDATE;TZID=Europe/Berlin:20261026T100000\r\nORGANIZER:mailto:boss@x.com\r\n\
ATTENDEE;PARTSTAT=ACCEPTED;CN=Me:mailto:me@x.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
		let b = import_body(object).unwrap();
		assert_eq!(b["iCalUID"], "x@y");
		assert_eq!(
			b["start"],
			json!({ "dateTime": "2026-10-19T10:00:00", "timeZone": "Europe/Berlin" })
		);
		assert_eq!(b["end"]["dateTime"], "2026-10-19T08:30:00Z");
		assert_eq!(b["end"]["timeZone"], "Europe/Berlin");
		assert_eq!(
			b["recurrence"],
			json!([
				"RRULE:FREQ=WEEKLY;BYDAY=MO",
				"EXDATE;TZID=Europe/Berlin:20261026T100000"
			])
		);
		assert_eq!(b["attendees"][0]["responseStatus"], "accepted");
		let all_day = import_body("BEGIN:VEVENT\nUID:d\nDTSTART;VALUE=DATE:20261224\nSUMMARY:Eve\nEND:VEVENT").unwrap();
		assert_eq!(
			(all_day["start"]["date"].as_str(), all_day["end"]["date"].as_str()),
			(Some("2026-12-24"), Some("2026-12-25"))
		);
		assert!(import_body("BEGIN:VEVENT\nUID:c\nSTATUS:CANCELLED\nDTSTART:20261019T100000Z\nEND:VEVENT").is_none());
	}

	#[test]
	fn errors_say_what_to_do() {
		let resp = |status, body: &str| http::Response {
			status,
			headers: vec![],
			body: body.into(),
		};
		assert!(matches!(check(resp(401, "{}")), Err(ApiError::Auth(_))));
		let disabled = r#"{"error":{"code":403,"message":"Google Calendar API has not been used","errors":[{"reason":"accessNotConfigured"}]}}"#;
		match check(resp(403, disabled)) {
			Err(ApiError::Other(m)) => assert!(m.contains("console.cloud.google.com")),
			_ => panic!("disabled API"),
		}
		assert_eq!(check(resp(204, "")).unwrap(), Value::Null);
	}
}
