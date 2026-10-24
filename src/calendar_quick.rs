//! Quick add: one line of plain English into an event, the way Fantastical
//! reads it — "Lunch with anna@x.com tomorrow 1pm", "Standup every weekday
//! 9:30-9:45 +meet", "Dentist Oct 24 3pm for 45m @Main St 4", "Offsite
//! 24/11-26/11". Words that set the date, time, length, repeat, guests or
//! place are taken out; what is left is the title. Local and instant: the
//! pane previews the result as the user types.

use crate::calendar::Draft;
use crate::calendar_time::{self as t, Date};

#[derive(Debug, Clone, PartialEq)]
pub struct Quick {
	pub title: String,
	pub all_day: bool,
	pub date: Date,
	/// Minute of the day; 0 for all-day events.
	pub start: u32,
	/// Last day (all-day, inclusive) or the day the event ends.
	pub end_date: Date,
	pub end: u32,
	pub recur: String,
	pub attendees: Vec<String>,
	pub location: String,
	pub video: bool,
}

impl Quick {
	pub fn draft(&self, calendar: &str) -> Draft {
		let (start, end) = if self.all_day {
			(
				t::local_midnight(self.date),
				t::local_midnight(self.end_date.add_days(1)),
			)
		} else {
			(t::local_ts(self.date, self.start), t::local_ts(self.end_date, self.end))
		};
		Draft {
			key: String::new(),
			calendar: calendar.to_string(),
			title: self.title.clone(),
			start,
			end,
			all_day: self.all_day,
			start_date: self.date.iso(),
			end_date: self.end_date.add_days(1).iso(),
			location: self.location.clone(),
			description: String::new(),
			attendees: self.attendees.clone(),
			video: self.video,
			recur: self.recur.clone(),
		}
	}
}

const MONTHS: [&str; 12] = [
	"jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn month(word: &str) -> Option<u32> {
	let w = word.trim_end_matches('.');
	if w.len() < 3 {
		return None;
	}
	let full = [
		"january",
		"february",
		"march",
		"april",
		"may",
		"june",
		"july",
		"august",
		"september",
		"october",
		"november",
		"december",
	];
	full.iter()
		.position(|m| m.starts_with(w) && (w.len() >= 3))
		.or_else(|| (w == "sept").then_some(8))
		.map(|i| i as u32 + 1)
		.filter(|_| MONTHS.iter().any(|m| w.starts_with(m)))
}

fn weekday(word: &str) -> Option<u32> {
	let w = word.trim_end_matches(['.', ',']);
	let full = [
		"sunday",
		"monday",
		"tuesday",
		"wednesday",
		"thursday",
		"friday",
		"saturday",
	];
	if w.len() < 3 {
		return None;
	}
	full.iter()
		.position(|d| d.starts_with(w))
		.filter(|i| w.starts_with(WEEKDAYS[*i]) || w == "tues" || w == "thur" || w == "thurs")
		.map(|i| i as u32)
}

/// `24`, `24th`, `1st` as a day of the month.
fn day_number(word: &str) -> Option<u32> {
	let w = word.trim_end_matches(['.', ',']);
	let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
	let suffix = &w[digits.len()..];
	if !matches!(suffix, "" | "st" | "nd" | "rd" | "th") {
		return None;
	}
	digits.parse().ok().filter(|d| (1..=31).contains(d))
}

/// A clock time: minutes, and whether am/pm was given.
fn clock(word: &str) -> Option<(u32, Option<bool>)> {
	let w = word.trim_end_matches([',', '.']).to_ascii_lowercase();
	let (body, pm) = if let Some(b) = w.strip_suffix("pm").or_else(|| w.strip_suffix("p")) {
		(b.to_string(), Some(true))
	} else if let Some(b) = w.strip_suffix("am").or_else(|| w.strip_suffix("a")) {
		(b.to_string(), Some(false))
	} else {
		(w.clone(), None)
	};
	match body.as_str() {
		"noon" | "midday" => return Some((12 * 60, Some(true))),
		"midnight" => return Some((0, Some(false))),
		_ => {}
	}
	let (h, m) = match body.split_once([':', '.']) {
		Some((h, m)) if m.len() == 2 => (h.parse::<u32>().ok()?, m.parse::<u32>().ok().filter(|m| *m < 60)?),
		Some(_) => return None,
		None => (body.parse::<u32>().ok()?, 0),
	};
	if body.is_empty() || h > 23 || (pm.is_some() && (h == 0 || h > 12)) {
		return None;
	}
	let h = match pm {
		Some(true) if h < 12 => h + 12,
		Some(false) if h == 12 => 0,
		_ => h,
	};
	Some((h * 60 + m, pm))
}

/// Whether a bare clock (no am/pm, no colon) should be read as one.
fn explicit(word: &str) -> bool {
	word.contains(':') || word.chars().last().is_some_and(|c| c.is_ascii_alphabetic())
}

/// Bare hours mean working hours: 1–7 are afternoons.
fn assume_pm(minute: u32, pm: Option<bool>, had_colon_24h: bool) -> u32 {
	if pm.is_none() && !had_colon_24h && (60..8 * 60).contains(&minute) {
		minute + 12 * 60
	} else {
		minute
	}
}

/// A range `a-b` of clocks; the end's am/pm carries over to a start without one.
fn clock_range(a: &str, b: &str) -> Option<(u32, u32)> {
	let (s, spm) = clock(a)?;
	let (e, epm) = clock(b)?;
	let s = match (spm, epm) {
		(None, Some(true)) if s < 12 * 60 && s + 12 * 60 < e => s + 12 * 60,
		(None, None) if !a.contains(':') || s < 8 * 60 => assume_pm(s, None, a.contains(':') && s >= 13 * 60),
		_ => s,
	};
	let e = match epm {
		None if e <= s && e + 12 * 60 > s => e + 12 * 60,
		_ => e,
	};
	Some((s, e))
}

/// Minutes in `for 45m`, `90 min`, `1.5h`, `2 hours`.
fn duration(num: &str, unit: Option<&str>) -> Option<u32> {
	let num = num.to_ascii_lowercase();
	let split = num.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(num.len());
	let (n, glued) = num.split_at(split);
	let n: f64 = n.parse().ok()?;
	let unit = if glued.is_empty() { unit? } else { glued };
	let per = match unit.trim_end_matches(['.', ',']) {
		"m" | "min" | "mins" | "minute" | "minutes" => 1.0,
		"h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
		"d" | "day" | "days" => 24.0 * 60.0,
		_ => return None,
	};
	let total = (n * per).round();
	(total > 0.0 && total <= 60.0 * 24.0 * 31.0).then_some(total as u32)
}

/// A date written with digits: `2026-10-24`, `24/10`, `24.10.2026`, `10/24`.
fn numeric_date(word: &str, today: Date) -> Option<Date> {
	let w = word.trim_end_matches([',', '.']);
	if let Some(d) = Date::parse(w).filter(|_| w.contains('-') && w.len() == 10) {
		return Some(d);
	}
	let parts: Vec<&str> = w.split(['/', '.']).collect();
	if !(2..=3).contains(&parts.len())
		|| !parts
			.iter()
			.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
	{
		return None;
	}
	let a: u32 = parts[0].parse().ok()?;
	let b: u32 = parts[1].parse().ok()?;
	// Day first, unless that cannot be.
	let (d, m) = if b > 12 && a <= 12 { (b, a) } else { (a, b) };
	let explicit_year = parts
		.get(2)
		.and_then(|y| y.parse::<i32>().ok())
		.map(|y| if y < 100 { 2000 + y } else { y });
	let y = explicit_year.unwrap_or(today.y);
	let valid = |y: i32| (1..=12).contains(&m) && d >= 1 && d <= Date::days_in_month(y, m);
	if !valid(y) {
		return None;
	}
	let date = Date::new(y, m, d);
	Some(if explicit_year.is_none() && date < today {
		Date::new(y + 1, m, d.min(Date::days_in_month(y + 1, m)))
	} else {
		date
	})
}

/// The next `wd` on or after `today` (after, with `next`: in the next week).
fn next_weekday(today: Date, wd: u32, next: bool) -> Date {
	let ahead = (wd + 7 - today.weekday()) % 7;
	if next {
		// "next Monday": the Monday of next week (weeks start on Monday).
		let monday_next = today.add_days(7 - ((today.weekday() + 6) % 7) as i64);
		monday_next.add_days(((wd + 6) % 7) as i64)
	} else {
		today.add_days(ahead as i64)
	}
}

/// Read `text` as of local `today` at `now_minute`.
pub fn parse(text: &str, today: Date, now_minute: u32) -> Quick {
	let words: Vec<&str> = text.split_whitespace().collect();
	let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
	let mut used = vec![false; words.len()];
	let mut date: Option<Date> = None;
	let mut end_date: Option<Date> = None;
	let mut start: Option<u32> = None;
	let mut end: Option<u32> = None;
	let mut length: Option<u32> = None;
	let mut all_day = false;
	let mut recur = String::new();
	let mut attendees = Vec::new();
	let mut location = String::new();
	let mut video = false;
	let mut default_time: Option<u32> = None;

	let lw = |i: usize| lower.get(i).map(String::as_str).unwrap_or("");
	let mut i = 0;
	while i < words.len() {
		let w = lw(i);
		let word = w.trim_end_matches([',', '.', ';']);
		let mut take = 0;

		// Place: `@Place …` to the end of the line.
		if words[i].starts_with('@') && words[i].len() > 1 && !words[i][1..].contains('@') {
			location = words[i..].join(" ")[1..].trim().to_string();
			for u in used.iter_mut().skip(i) {
				*u = true;
			}
			break;
		}
		if word.contains('@') && word.contains('.') && !word.starts_with('@') {
			attendees.push(word.trim_matches(['<', '>', '(', ')']).to_string());
			take = 1;
			if i > 0 && lw(i - 1) == "with" && !used[i - 1] {
				used[i - 1] = true;
			}
		} else if matches!(word, "+meet" | "+video" | "+zoom" | "+teams" | "+call") {
			video = true;
			take = 1;
		} else if word == "all" && lw(i + 1).trim_end_matches([',', '.']) == "day"
			|| word == "all-day"
			|| word == "allday"
		{
			all_day = true;
			take = if word == "all" { 2 } else { 1 };
		} else if word == "today" {
			date = Some(today);
			take = 1;
		} else if word == "tonight" {
			date = Some(today);
			default_time = Some(19 * 60);
			take = 1;
		} else if matches!(word, "tomorrow" | "tmrw" | "tmr" | "tomorow") {
			date = Some(today.add_days(1));
			take = 1;
		} else if word == "day" && lw(i + 1) == "after" && lw(i + 2).starts_with("tomorrow") {
			date = Some(today.add_days(2));
			take = 3;
		} else if matches!(word, "daily") {
			recur = "daily".into();
			take = 1;
		} else if matches!(word, "weekly") {
			recur = "weekly".into();
			take = 1;
		} else if matches!(word, "monthly") {
			recur = "monthly".into();
			take = 1;
		} else if matches!(word, "yearly" | "annually") {
			recur = "yearly".into();
			take = 1;
		} else if word == "every" || word == "each" {
			let next = lw(i + 1).trim_end_matches([',', '.']);
			take = 2;
			match next {
				"day" => recur = "daily".into(),
				"weekday" | "weekdays" | "workday" | "workdays" => recur = "weekdays".into(),
				"week" => recur = "weekly".into(),
				"month" => recur = "monthly".into(),
				"year" => recur = "yearly".into(),
				n => match weekday(n) {
					Some(wd) => {
						recur = "weekly".into();
						date = Some(next_weekday(today, wd, false));
					}
					None => take = 0,
				},
			}
		} else if word == "next" && lw(i + 1).trim_end_matches([',', '.']) == "week" {
			date = Some(next_weekday(today, 1, true));
			take = 2;
		} else if word == "next" && weekday(lw(i + 1)).is_some() {
			date = Some(next_weekday(today, weekday(lw(i + 1)).unwrap(), true));
			take = 2;
		} else if matches!(word, "weekend" | "this-weekend") {
			date = Some(next_weekday(today, 6, false));
			if i > 0 && lw(i - 1) == "this" {
				used[i - 1] = true;
			}
			take = 1;
		} else if let Some(wd) = weekday(word) {
			date = Some(next_weekday(today, wd, false));
			take = 1;
		} else if word == "in" && lw(i + 1).parse::<u32>().is_ok() {
			let n: i64 = lw(i + 1).parse().unwrap_or(0);
			match lw(i + 2).trim_end_matches([',', '.']) {
				"day" | "days" => {
					date = Some(today.add_days(n));
					take = 3;
				}
				"week" | "weeks" => {
					date = Some(today.add_days(7 * n));
					take = 3;
				}
				"month" | "months" => {
					date = Some(today.add_months(n as i32));
					take = 3;
				}
				u @ ("min" | "mins" | "minutes" | "minute" | "hour" | "hours" | "h" | "m") => {
					if let Some(minutes) = duration(&n.to_string(), Some(u)) {
						let total = now_minute + minutes;
						date = Some(today.add_days(i64::from(total / 1440)));
						start = Some(total % 1440);
						take = 3;
					}
				}
				_ => {}
			}
		} else if word == "for" && duration(lw(i + 1), Some(lw(i + 2))).is_some() {
			let glued = lw(i + 1).ends_with(|c: char| c.is_ascii_alphabetic());
			length = duration(lw(i + 1), Some(lw(i + 2)));
			take = if glued { 2 } else { 3 };
		} else if let Some(m) = month(word) {
			// `Oct 24`, `October 24th, 2027`
			if let Some(d) = day_number(lw(i + 1)) {
				let year = lw(i + 2)
					.trim_end_matches([',', '.'])
					.parse::<i32>()
					.ok()
					.filter(|y| *y > 2000);
				take = if year.is_some() { 3 } else { 2 };
				date = dated(today, year, m, d);
			}
		} else if let (Some(d), Some(m)) = (day_number(word), month(lw(i + 1))) {
			// `24 Oct`, `24th of October` handled below
			let year = lw(i + 2)
				.trim_end_matches([',', '.'])
				.parse::<i32>()
				.ok()
				.filter(|y| *y > 2000);
			take = if year.is_some() { 3 } else { 2 };
			date = dated(today, year, m, d);
		} else if let (Some(d), "of", Some(m)) = (day_number(word), lw(i + 1), month(lw(i + 2))) {
			take = 3;
			date = dated(today, None, m, d);
		} else if word.contains(['-', '–']) && !word.starts_with('-') {
			// `3-4pm`, `9:30-10`, `24/11-26/11`
			let (a, b) = word.split_once(['-', '–']).unwrap();
			if let Some((s, e)) =
				clock_range(a, b).filter(|_| explicit(a) || explicit(b) || lw(i.saturating_sub(1)) == "at")
			{
				start = Some(s);
				end = Some(e);
				take = 1;
			} else if let (Some(a), Some(b)) = (numeric_date(a, today), numeric_date(b, today)) {
				date = Some(a);
				end_date = Some(b);
				all_day = true;
				take = 1;
			} else if let Some(d) = numeric_date(word, today) {
				date = Some(d);
				take = 1;
			}
		} else if let Some(d) = numeric_date(word, today) {
			date = Some(d);
			take = 1;
		} else if matches!(word, "at" | "from" | "@") && clock(lw(i + 1)).is_some() {
			let (s, pm) = clock(lw(i + 1)).unwrap();
			let mut s = s;
			let mut n = 2;
			if pm.is_none() && matches!(lw(i + 2), "pm" | "am") {
				s = clock(&format!("{}{}", lw(i + 1), lw(i + 2))).map_or(s, |c| c.0);
				n = 3;
			} else if pm.is_none() {
				s = assume_pm(s, None, lw(i + 1).contains(':') && s >= 13 * 60);
			}
			start = Some(s);
			take = n;
		} else if let Some((s, pm)) = clock(word).filter(|_| explicit(word)) {
			let mut s = s;
			take = 1;
			if pm.is_none() && matches!(lw(i + 1), "pm" | "am") {
				s = clock(&format!("{word}{}", lw(i + 1))).map_or(s, |c| c.0);
				take = 2;
			}
			start = Some(s);
		} else if matches!(word, "to" | "until" | "till" | "-" | "–")
			&& start.is_some()
			&& end.is_none()
			&& clock(lw(i + 1)).is_some()
		{
			let raw = lw(i + 1);
			let s = start.unwrap();
			if let Some((s2, e)) = clock_range(&min_to_clock(s), raw) {
				start = Some(s2.max(s));
				end = Some(if e <= s { e + 12 * 60 } else { e });
			}
			take = 2;
		} else if matches!(word, "noon" | "midday" | "midnight") {
			start = clock(word).map(|c| c.0);
			take = 1;
			if i > 0 && lw(i - 1) == "at" {
				used[i - 1] = true;
			}
		} else if matches!(word, "morning") {
			default_time = Some(9 * 60);
			take = 1;
		} else if matches!(word, "afternoon") {
			default_time = Some(14 * 60);
			take = 1;
		} else if matches!(word, "evening") {
			default_time = Some(18 * 60);
			take = 1;
		}

		if take > 0 {
			for u in used.iter_mut().skip(i).take(take) {
				*u = true;
			}
			// The connector before a date or time goes with it.
			if i > 0 && !used[i - 1] && matches!(lw(i - 1), "on" | "at" | "from" | "by" | "this" | "starting") {
				used[i - 1] = true;
			}
			i += take;
		} else {
			i += 1;
		}
	}

	let mut title_words: Vec<&str> = words.iter().zip(&used).filter(|(_, u)| !**u).map(|(w, _)| *w).collect();
	// Dangling connectors left at the edges.
	while title_words.last().is_some_and(|w| {
		matches!(
			w.to_ascii_lowercase().as_str(),
			"with" | "at" | "on" | "for" | "in" | "from" | "to" | "and" | "-" | "–"
		)
	}) {
		title_words.pop();
	}
	while title_words
		.first()
		.is_some_and(|w| matches!(w.to_ascii_lowercase().as_str(), "on" | "at" | "-" | "–"))
	{
		title_words.remove(0);
	}
	let title = title_words.join(" ").trim_matches([',', ' ']).to_string();

	let start = start.or(default_time);
	let timed = start.is_some() && !all_day;
	let mut date = date.unwrap_or(today);
	// A time already past today, with no day given, means tomorrow.
	if timed
		&& date == today
		&& start.is_some_and(|s| s < now_minute)
		&& !text.to_ascii_lowercase().contains("today")
		&& recur.is_empty()
	{
		date = today.add_days(1);
	}
	if recur == "weekdays" && matches!(date.weekday(), 0 | 6) {
		date = next_weekday(date, 1, false);
	}
	if timed {
		let s = start.unwrap();
		let len = match (end, length) {
			(Some(e), _) => e.saturating_sub(s).max(5),
			(None, Some(l)) => l,
			(None, None) => 60,
		};
		let total = s + len;
		Quick {
			title,
			all_day: false,
			date,
			start: s,
			end_date: date.add_days(i64::from(total / 1440)),
			end: total % 1440,
			recur,
			attendees,
			location,
			video,
		}
	} else {
		let days = length.map_or(1, |l| (l / 1440).max(1));
		let last = end_date
			.filter(|e| *e >= date)
			.unwrap_or(date.add_days(i64::from(days) - 1));
		Quick {
			title,
			all_day: true,
			date,
			start: 0,
			end_date: last,
			end: 0,
			recur,
			attendees,
			location,
			video,
		}
	}
}

fn min_to_clock(m: u32) -> String {
	format!("{}:{:02}", m / 60, m % 60)
}

/// Day `d` of month `m`: this year, or next year once it has passed.
fn dated(today: Date, year: Option<i32>, m: u32, d: u32) -> Option<Date> {
	let y = year.unwrap_or(today.y);
	if d > Date::days_in_month(y, m) {
		return None;
	}
	let date = Date::new(y, m, d);
	Some(if year.is_none() && date < today {
		Date::new(y + 1, m, d.min(Date::days_in_month(y + 1, m)))
	} else {
		date
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	// Thursday 22 October 2026, 10:30.
	const TODAY: Date = Date { y: 2026, m: 10, d: 22 };
	const NOW: u32 = 10 * 60 + 30;

	fn p(s: &str) -> Quick {
		parse(s, TODAY, NOW)
	}

	fn hm(h: u32, m: u32) -> u32 {
		h * 60 + m
	}

	#[test]
	fn times_and_days() {
		let q = p("Lunch with anna@x.com tomorrow 1pm");
		assert_eq!(q.title, "Lunch");
		assert_eq!(
			(q.date, q.start, q.end),
			(Date::new(2026, 10, 23), hm(13, 0), hm(14, 0))
		);
		assert_eq!(q.attendees, ["anna@x.com"]);

		let q = p("Call Bob at 3");
		assert_eq!((q.title.as_str(), q.date, q.start), ("Call Bob", TODAY, hm(15, 0)));

		let q = p("Review 9am");
		assert_eq!(q.date, Date::new(2026, 10, 23), "9am has passed today");

		let q = p("Dentist Oct 24 3pm for 45m");
		assert_eq!(
			(q.title.as_str(), q.date, q.start, q.end),
			("Dentist", Date::new(2026, 10, 24), hm(15, 0), hm(15, 45))
		);

		let q = p("Sync 15:00-16:30 on friday");
		assert_eq!(
			(q.title.as_str(), q.date, q.start, q.end),
			("Sync", Date::new(2026, 10, 23), hm(15, 0), hm(16, 30))
		);

		let q = p("Planning 11-1pm next monday");
		assert_eq!(
			(q.date, q.start, q.end),
			(Date::new(2026, 10, 26), hm(11, 0), hm(13, 0))
		);

		let q = p("Planning from 2 to 4 tuesday");
		assert_eq!(
			(q.title.as_str(), q.date, q.start, q.end),
			("Planning", Date::new(2026, 10, 27), hm(14, 0), hm(16, 0))
		);

		let q = p("Drinks tonight");
		assert_eq!((q.date, q.start), (TODAY, hm(19, 0)));

		let q = p("Focus in 2 hours for 90 min");
		assert_eq!((q.start, q.end), (hm(12, 30), hm(14, 0)));

		let q = p("Call at noon today");
		assert_eq!((q.title.as_str(), q.date, q.start), ("Call", TODAY, hm(12, 0)));
	}

	#[test]
	fn all_day_ranges_repeats_and_places() {
		let q = p("Dentist tomorrow");
		assert!(q.all_day);
		assert_eq!((q.date, q.end_date), (Date::new(2026, 10, 23), Date::new(2026, 10, 23)));

		let q = p("Offsite 24/11-26/11");
		assert!(q.all_day);
		assert_eq!(
			(q.title.as_str(), q.date, q.end_date),
			("Offsite", Date::new(2026, 11, 24), Date::new(2026, 11, 26))
		);

		let q = p("Standup every weekday 9:30-9:45 +meet");
		assert_eq!(
			(q.title.as_str(), q.recur.as_str(), q.start, q.end, q.video),
			("Standup", "weekdays", hm(9, 30), hm(9, 45), true)
		);
		assert_eq!(q.date, TODAY, "repeats start today even when the time has passed");

		let q = p("1:1 every tuesday 4pm @Room 5, 2nd floor");
		assert_eq!(
			(q.title.as_str(), q.recur.as_str(), q.date),
			("1:1", "weekly", Date::new(2026, 10, 27))
		);
		assert_eq!(q.location, "Room 5, 2nd floor");

		let q = p("Trip 2027-03-01 for 3 days");
		assert_eq!((q.date, q.end_date), (Date::new(2027, 3, 1), Date::new(2027, 3, 3)));

		let q = p("Birthday 5 Jan");
		assert_eq!(q.date, Date::new(2027, 1, 5), "a date already past is next year's");

		let d = p("Party Saturday 8pm").draft("cal");
		assert_eq!(d.end - d.start, 3600);
		assert_eq!(d.title, "Party");
		let d = p("Holiday all day monday").draft("cal");
		assert!(d.all_day);
		assert_eq!(
			(d.start_date.as_str(), d.end_date.as_str()),
			("2026-10-26", "2026-10-27")
		);
	}

	#[test]
	fn titles_keep_words_that_only_look_like_dates() {
		assert_eq!(p("Read 2 chapters").title, "Read 2 chapters");
		assert_eq!(p("Mark's farewell").title, "Mark's farewell");
		assert_eq!(p("May sync with design").title, "May sync with design");
	}
}
