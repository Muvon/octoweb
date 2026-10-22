//! Microsoft Graph calendars for Outlook.com and Microsoft 365 accounts.
//! `calendarView` returns occurrences of recurring series expanded, each with
//! its own id; times come in UTC.

use serde_json::{json, Value};

use crate::calendar::{self, ApiError, Attendee, CalEvent, Calendar, Ctx, Draft};
use crate::calendar_time::{self as t, Date};
use crate::http::{self, Request};

const API: &str = "https://graph.microsoft.com/v1.0";

fn check(resp: http::Response) -> Result<Value, ApiError> {
	if resp.ok() {
		return Ok(if resp.body.trim().is_empty() {
			Value::Null
		} else {
			resp.json()?
		});
	}
	let body: Value = serde_json::from_str(&resp.body).unwrap_or(Value::Null);
	let message = body["error"]["message"].as_str().unwrap_or("").to_string();
	if resp.status == 401 || (resp.status == 403 && body["error"]["code"].as_str() == Some("ErrorAccessDenied")) {
		return Err(ApiError::Auth(if message.is_empty() {
			"Microsoft sign-in expired: connect the calendar again".into()
		} else {
			format!("Connect the calendar again: {message}")
		}));
	}
	Err(ApiError::Other(if message.is_empty() {
		format!("Microsoft Graph answered {}", resp.status)
	} else {
		message
	}))
}

/// Graph's named calendar colours, as hex.
fn color(c: &Value) -> String {
	if let Some(hex) = c["hexColor"].as_str().filter(|h| h.starts_with('#')) {
		return hex.to_string();
	}
	match c["color"].as_str().unwrap_or("auto") {
		"lightBlue" => "#4f9fe8",
		"lightGreen" => "#4ec36d",
		"lightOrange" => "#f7a33e",
		"lightGray" => "#9b9b9b",
		"lightYellow" => "#e6c32f",
		"lightTeal" => "#3bbfb0",
		"lightPink" => "#e872a8",
		"lightBrown" => "#a87a50",
		"lightRed" => "#e8564f",
		_ => "#0078d4",
	}
	.to_string()
}

async fn get_all(ctx: &Ctx, first: String, prefer_utc: bool) -> Result<Vec<Value>, ApiError> {
	let mut out = Vec::new();
	let mut url = first;
	loop {
		let mut req = ctx.apply(Request::new("GET", &url));
		if prefer_utc {
			req = req.header("Prefer", "outlook.timezone=\"UTC\"");
		}
		let body = check(http::send(req).await?)?;
		out.extend(body["value"].as_array().cloned().unwrap_or_default());
		match body["@odata.nextLink"].as_str() {
			Some(next) => url = next.to_string(),
			None => return Ok(out),
		}
	}
}

pub async fn calendars(account: &str, ctx: &Ctx) -> Result<Vec<Calendar>, ApiError> {
	let items = get_all(ctx, format!("{API}/me/calendars?$top=100"), false).await?;
	Ok(items
		.iter()
		.filter_map(|c| {
			let id = c["id"].as_str()?.to_string();
			Some(Calendar {
				key: format!("{account}/{id}"),
				account: account.to_string(),
				name: c["name"].as_str().unwrap_or("Calendar").to_string(),
				color: color(c),
				primary: c["isDefaultCalendar"].as_bool() == Some(true),
				writable: c["canEdit"].as_bool() != Some(false),
				url: String::new(),
				id,
			})
		})
		.collect())
}

/// An all-day event's date: midnight in UTC when the header was honoured,
/// otherwise the nearest midnight.
fn day(ts: i64) -> Date {
	Date::from_days((ts + 12 * 3600).div_euclid(86_400))
}

pub fn occurrence(e: &Value, cal: &Calendar, address: &str) -> Option<CalEvent> {
	if e["isCancelled"].as_bool() == Some(true) {
		return None;
	}
	let id = e["id"].as_str()?.to_string();
	let start = t::parse_rfc3339(
		e["start"]["dateTime"].as_str()?,
		e["start"]["timeZone"].as_str().unwrap_or("UTC"),
	)?;
	let end = t::parse_rfc3339(
		e["end"]["dateTime"].as_str()?,
		e["end"]["timeZone"].as_str().unwrap_or("UTC"),
	)?;
	let all_day = e["isAllDay"].as_bool() == Some(true);
	let (start, end, start_date, end_date) = if all_day {
		let (a, b) = (day(start), day(end).max(day(start).add_days(1)));
		(t::local_midnight(a), t::local_midnight(b), a.iso(), b.iso())
	} else {
		(start, end, String::new(), String::new())
	};
	let organizer = e["organizer"]["emailAddress"]["address"]
		.as_str()
		.unwrap_or_default()
		.to_ascii_lowercase();
	let is_organizer = e["isOrganizer"].as_bool() == Some(true);
	let mut attendees: Vec<Attendee> = e["attendees"]
		.as_array()
		.into_iter()
		.flatten()
		.filter(|a| a["type"].as_str() != Some("resource"))
		.map(|a| {
			let email = a["emailAddress"]["address"]
				.as_str()
				.unwrap_or_default()
				.to_ascii_lowercase();
			Attendee {
				me: email == address,
				name: a["emailAddress"]["name"].as_str().unwrap_or_default().to_string(),
				status: calendar::status_word(a["status"]["response"].as_str().unwrap_or("")),
				organizer: email == organizer,
				optional: a["type"].as_str() == Some("optional"),
				email,
			}
		})
		.collect();
	if !attendees.is_empty() && !organizer.is_empty() && !attendees.iter().any(|a| a.organizer) {
		attendees.insert(
			0,
			Attendee {
				me: is_organizer,
				name: e["organizer"]["emailAddress"]["name"]
					.as_str()
					.unwrap_or_default()
					.to_string(),
				status: "accepted".into(),
				organizer: true,
				optional: false,
				email: organizer.clone(),
			},
		);
	}
	let me = if is_organizer || attendees.is_empty() {
		String::new()
	} else {
		calendar::status_word(e["responseStatus"]["response"].as_str().unwrap_or(""))
	};
	let show_as = e["showAs"].as_str().unwrap_or("busy");
	let location = e["location"]["displayName"].as_str().unwrap_or_default();
	let body = e["body"]["content"].as_str().unwrap_or_default();
	let conference = format!(
		"{} {}",
		e["onlineMeeting"]["joinUrl"].as_str().unwrap_or_default(),
		e["onlineMeetingUrl"].as_str().unwrap_or_default()
	);
	Some(CalEvent {
		key: format!("{}/{id}", cal.key),
		account: cal.account.clone(),
		calendar: cal.key.clone(),
		series: e["seriesMasterId"].as_str().unwrap_or_default().to_string(),
		title: e["subject"]
			.as_str()
			.filter(|s| !s.trim().is_empty())
			.unwrap_or("(No title)")
			.trim()
			.to_string(),
		start,
		end,
		all_day,
		start_date,
		end_date,
		location: location.to_string(),
		description: calendar::plain(body),
		link: e["webLink"].as_str().unwrap_or_default().to_string(),
		join: calendar::find_join(&[&conference, location, body]),
		organizer,
		me,
		busy: !matches!(show_as, "free" | "workingElsewhere"),
		recurring: e["seriesMasterId"].is_string(),
		editable: cal.writable && is_organizer,
		kind: if show_as == "oof" { "ooo".into() } else { String::new() },
		attendees,
		etag: String::new(),
		recurrence_id: String::new(),
		id,
	})
}

pub async fn events(address: &str, cal: &Calendar, ctx: &Ctx, from: i64, to: i64) -> Result<Vec<CalEvent>, ApiError> {
	let url = format!(
		"{API}/me/calendars/{}/calendarView?startDateTime={}&endDateTime={}&$top=250",
		http::encode(&cal.id),
		http::encode(&t::rfc3339_utc(from)),
		http::encode(&t::rfc3339_utc(to)),
	);
	let items = get_all(ctx, url, true).await?;
	Ok(items.iter().filter_map(|e| occurrence(e, cal, address)).collect())
}

pub fn body(draft: &Draft, creating: bool) -> Value {
	let when = |ts: i64, date: &str| {
		if draft.all_day {
			json!({ "dateTime": format!("{date}T00:00:00"), "timeZone": t::local_zone() })
		} else {
			json!({ "dateTime": t::rfc3339_utc(ts).trim_end_matches('Z'), "timeZone": "UTC" })
		}
	};
	let mut body = json!({
		"subject": draft.title.trim(),
		"body": { "contentType": "text", "content": draft.description },
		"start": when(draft.start, &draft.start_date),
		"end": when(draft.end, &draft.end_date),
		"isAllDay": draft.all_day,
		"location": { "displayName": draft.location.trim() },
		"attendees": draft.attendees.iter().map(|a| json!({ "emailAddress": { "address": a.trim() }, "type": "required" })).collect::<Vec<_>>(),
	});
	if draft.video {
		body["isOnlineMeeting"] = json!(true);
		body["onlineMeetingProvider"] = json!("teamsForBusiness");
	}
	if creating {
		let date = if draft.all_day {
			Date::parse(&draft.start_date)
		} else {
			Some(t::local_date(draft.start))
		};
		if let (Some(date), true) = (date, !draft.recur.is_empty()) {
			const DAYS: [&str; 7] = [
				"sunday",
				"monday",
				"tuesday",
				"wednesday",
				"thursday",
				"friday",
				"saturday",
			];
			let pattern = match draft.recur.as_str() {
				"daily" => json!({ "type": "daily", "interval": 1 }),
				"weekdays" => {
					json!({ "type": "weekly", "interval": 1, "daysOfWeek": ["monday", "tuesday", "wednesday", "thursday", "friday"] })
				}
				"weekly" => json!({ "type": "weekly", "interval": 1, "daysOfWeek": [DAYS[date.weekday() as usize]] }),
				"monthly" => json!({ "type": "absoluteMonthly", "interval": 1, "dayOfMonth": date.d }),
				_ => json!({ "type": "absoluteYearly", "interval": 1, "dayOfMonth": date.d, "month": date.m }),
			};
			body["recurrence"] = json!({ "pattern": pattern, "range": { "type": "noEnd", "startDate": date.iso() } });
		}
	}
	body
}

pub async fn save(cal: &Calendar, ctx: &Ctx, draft: &Draft, existing: Option<&CalEvent>) -> Result<(), ApiError> {
	match existing {
		None => {
			let url = format!("{API}/me/calendars/{}/events", http::encode(&cal.id));
			check(http::send(ctx.apply(Request::new("POST", &url)).json(&body(draft, true))).await?)?;
		}
		Some(e) => {
			let url = format!("{API}/me/events/{}", http::encode(&e.id));
			let mut patch = body(draft, false);
			// Plain text in the pane: an unchanged body keeps its formatting.
			if calendar::plain(&draft.description) == e.description {
				patch.as_object_mut().expect("object").remove("body");
			}
			check(http::send(ctx.apply(Request::new("PATCH", &url)).json(&patch)).await?)?;
		}
	}
	Ok(())
}

pub async fn delete(ctx: &Ctx, event: &CalEvent, all: bool) -> Result<(), ApiError> {
	let id = if all && !event.series.is_empty() {
		&event.series
	} else {
		&event.id
	};
	let resp = http::send(ctx.apply(Request::new("DELETE", &format!("{API}/me/events/{}", http::encode(id))))).await?;
	if resp.status == 404 {
		return Ok(());
	}
	check(resp).map(|_| ())
}

pub async fn rsvp(ctx: &Ctx, event: &CalEvent, response: &str) -> Result<(), ApiError> {
	let action = match response {
		"accepted" => "accept",
		"declined" => "decline",
		_ => "tentativelyAccept",
	};
	let url = format!("{API}/me/events/{}/{action}", http::encode(&event.id));
	check(
		http::send(
			ctx.apply(Request::new("POST", &url))
				.json(&json!({ "sendResponse": true })),
		)
		.await?,
	)?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn graph_events_become_occurrences() {
		let cal = Calendar {
			key: "k/AAA".into(),
			account: "k".into(),
			id: "AAA".into(),
			name: "Calendar".into(),
			color: "#0078d4".into(),
			primary: true,
			writable: true,
			url: String::new(),
		};
		let e = json!({
			"id": "OCC1", "subject": "Planning", "isAllDay": false, "isCancelled": false,
			"start": { "dateTime": "2026-10-22T08:00:00.0000000", "timeZone": "UTC" },
			"end": { "dateTime": "2026-10-22T09:00:00.0000000", "timeZone": "UTC" },
			"seriesMasterId": "SER", "showAs": "busy", "isOrganizer": false,
			"responseStatus": { "response": "tentativelyAccepted" },
			"organizer": { "emailAddress": { "name": "Lee", "address": "lee@x.com" } },
			"attendees": [ { "type": "required", "status": { "response": "none" }, "emailAddress": { "name": "Me", "address": "me@x.com" } } ],
			"onlineMeeting": { "joinUrl": "https://teams.microsoft.com/l/meetup-join/19%3a1" },
			"body": { "contentType": "html", "content": "<p>Plan Q4</p>" },
			"webLink": "https://outlook.office365.com/owa/?itemid=1"
		});
		let o = occurrence(&e, &cal, "me@x.com").unwrap();
		assert_eq!((o.start, o.end), (1_792_656_000, 1_792_659_600));
		assert_eq!(o.me, "tentative");
		assert_eq!(o.series, "SER");
		assert_eq!(o.join, "https://teams.microsoft.com/l/meetup-join/19%3a1");
		assert_eq!(o.attendees[0].email, "lee@x.com", "the organizer leads the guests");
		assert!(!o.editable);

		let all_day = json!({
			"id": "D", "subject": "Offsite", "isAllDay": true,
			"start": { "dateTime": "2026-10-22T00:00:00.0000000", "timeZone": "UTC" },
			"end": { "dateTime": "2026-10-23T00:00:00.0000000", "timeZone": "UTC" },
			"isOrganizer": true, "showAs": "oof"
		});
		let o = occurrence(&all_day, &cal, "me@x.com").unwrap();
		assert_eq!(
			(o.start_date.as_str(), o.end_date.as_str()),
			("2026-10-22", "2026-10-23")
		);
		assert_eq!(o.kind, "ooo");
		assert!(o.editable && o.me.is_empty());
	}

	#[test]
	fn drafts_carry_recurrence_and_teams() {
		let d = Draft {
			title: "1:1".into(),
			start: 1_792_656_000,
			end: 1_792_657_800,
			recur: "weekly".into(),
			video: true,
			..Default::default()
		};
		let b = body(&d, true);
		assert_eq!(b["start"]["dateTime"], "2026-10-22T08:00:00");
		assert_eq!(b["recurrence"]["pattern"]["daysOfWeek"][0], "thursday");
		assert_eq!(b["isOnlineMeeting"], true);
		assert!(body(&d, false).get("recurrence").is_none());
	}
}
