//! Dates and times for the calendar: civil-date arithmetic, local time through
//! libc, other zones through `NSTimeZone` (the system's tz database), and the
//! RFC 3339 / iCalendar forms the providers speak. Instants are Unix seconds.

use objc2_foundation::{NSDate, NSString, NSTimeZone};

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
	pub y: i32,
	pub m: u32,
	pub d: u32,
}

impl Date {
	pub fn new(y: i32, m: u32, d: u32) -> Self {
		Self { y, m, d }
	}

	/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
	pub fn days(self) -> i64 {
		let y = i64::from(self.y) - i64::from(self.m <= 2);
		let era = if y >= 0 { y } else { y - 399 } / 400;
		let yoe = y - era * 400;
		let m = i64::from(self.m);
		let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.d) - 1;
		let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
		era * 146_097 + doe - 719_468
	}

	pub fn from_days(days: i64) -> Self {
		let z = days + 719_468;
		let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
		let doe = z - era * 146_097;
		let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
		let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
		let mp = (5 * doy + 2) / 153;
		let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
		let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
		Self {
			y: (yoe + era * 400 + i64::from(m <= 2)) as i32,
			m,
			d,
		}
	}

	pub fn add_days(self, n: i64) -> Self {
		Self::from_days(self.days() + n)
	}

	/// 0 = Sunday … 6 = Saturday.
	pub fn weekday(self) -> u32 {
		(self.days() + 4).rem_euclid(7) as u32
	}

	pub fn days_in_month(y: i32, m: u32) -> u32 {
		match m {
			1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
			4 | 6 | 9 | 11 => 30,
			_ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
			_ => 28,
		}
	}

	/// The same day `n` months on, clamped to the month's length.
	pub fn add_months(self, n: i32) -> Self {
		let total = self.y * 12 + self.m as i32 - 1 + n;
		let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
		Self::new(y, m, self.d.min(Self::days_in_month(y, m)))
	}

	/// `YYYY-MM-DD`, or `YYYYMMDD` as iCalendar writes it.
	pub fn parse(s: &str) -> Option<Self> {
		let s = s.trim();
		let digits: String = s.chars().filter(|c| *c != '-').collect();
		if digits.len() != 8 || !digits.bytes().all(|b| b.is_ascii_digit()) {
			return None;
		}
		let date = Self::new(
			digits[..4].parse().ok()?,
			digits[4..6].parse().ok()?,
			digits[6..8].parse().ok()?,
		);
		(date.m >= 1 && date.m <= 12 && date.d >= 1 && date.d <= Self::days_in_month(date.y, date.m)).then_some(date)
	}

	pub fn iso(self) -> String {
		format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
	}

	pub fn compact(self) -> String {
		format!("{:04}{:02}{:02}", self.y, self.m, self.d)
	}
}

// ── Local time ────────────────────────────────────────────────────────────────

fn local_tm(ts: i64) -> libc::tm {
	// SAFETY: localtime_r fills the zeroed `tm` on this stack from `ts`.
	unsafe {
		let mut tm: libc::tm = std::mem::zeroed();
		libc::localtime_r(&(ts as libc::time_t), &mut tm);
		tm
	}
}

pub fn now() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_secs() as i64)
}

/// The local date and minute of day of `ts`.
pub fn local(ts: i64) -> (Date, u32) {
	let tm = local_tm(ts);
	(
		Date::new(tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32),
		(tm.tm_hour * 60 + tm.tm_min) as u32,
	)
}

pub fn local_date(ts: i64) -> Date {
	local(ts).0
}

/// The instant of local wall time `minute` (of the day) on `date`. A time
/// skipped by a DST change lands just after it, as mktime decides.
pub fn local_ts(date: Date, minute: u32) -> i64 {
	// SAFETY: mktime only reads and normalises the `tm` on this stack.
	unsafe {
		let mut tm: libc::tm = std::mem::zeroed();
		tm.tm_year = date.y - 1900;
		tm.tm_mon = date.m as i32 - 1;
		tm.tm_mday = date.d as i32;
		tm.tm_hour = (minute / 60) as i32;
		tm.tm_min = (minute % 60) as i32;
		tm.tm_isdst = -1;
		libc::mktime(&mut tm) as i64
	}
}

pub fn local_midnight(date: Date) -> i64 {
	local_ts(date, 0)
}

/// The IANA name of the Mac's time zone, e.g. `Europe/Berlin`.
pub fn local_zone() -> String {
	std::fs::read_link("/etc/localtime")
		.ok()
		.and_then(|p| {
			let p = p.to_string_lossy().to_string();
			p.split_once("zoneinfo/").map(|(_, name)| name.to_string())
		})
		.unwrap_or_else(|| "UTC".into())
}

// ── Other zones ───────────────────────────────────────────────────────────────

fn zone(tzid: &str) -> Option<objc2::rc::Retained<NSTimeZone>> {
	let mut name = tzid.trim().trim_matches('"').trim_start_matches('/');
	// Some clients prefix the IANA name: `/mozilla.org/20050126_1/Europe/Berlin`.
	loop {
		if name.is_empty() {
			return None;
		}
		if let Some(tz) = NSTimeZone::timeZoneWithName(&NSString::from_str(name)) {
			return Some(tz);
		}
		name = name.split_once('/')?.1;
	}
}

/// The IANA name the system knows `tzid` by (prefixes dropped), if any.
pub fn zone_name(tzid: &str) -> Option<String> {
	zone(tzid).map(|z| z.name().to_string())
}

/// Seconds east of UTC in zone `tzid` at instant `ts`.
fn zone_offset(tz: &NSTimeZone, ts: i64) -> i64 {
	let date = NSDate::dateWithTimeIntervalSince1970(ts as f64);
	tz.secondsFromGMTForDate(&date) as i64
}

/// The instant of wall time `minute` on `date` in zone `tzid`, or in local
/// time when the system does not know the zone (Windows names, say).
pub fn zone_ts(tzid: &str, date: Date, minute: u32) -> i64 {
	let Some(tz) = zone(tzid) else {
		return local_ts(date, minute);
	};
	let wall = date.days() * 86_400 + i64::from(minute) * 60;
	let first = wall - zone_offset(&tz, wall);
	wall - zone_offset(&tz, first)
}

// ── Wire formats ──────────────────────────────────────────────────────────────

/// RFC 3339 (`2026-10-22T10:00:00+02:00`, `…Z`, fractions allowed), or a
/// zoneless date-time read in `fallback_zone` (Graph's form with a
/// `timeZone` beside it).
pub fn parse_rfc3339(s: &str, fallback_zone: &str) -> Option<i64> {
	let s = s.trim();
	let (date, rest) = s.split_once('T')?;
	let date = Date::parse(date)?;
	let (time, offset) = match rest.find(['Z', 'z', '+', '-']) {
		Some(i) => (&rest[..i], Some(&rest[i..])),
		None => (rest, None),
	};
	let mut parts = time.split(':');
	let h: u32 = parts.next()?.parse().ok()?;
	let m: u32 = parts.next()?.parse().ok()?;
	let sec: f64 = parts.next().unwrap_or("0").parse().ok()?;
	let wall = date.days() * 86_400 + i64::from(h) * 3600 + i64::from(m) * 60 + sec as i64;
	match offset {
		Some(o) if o.eq_ignore_ascii_case("z") => Some(wall),
		Some(o) => {
			let sign = if o.starts_with('-') { -1 } else { 1 };
			let digits: String = o[1..].chars().filter(char::is_ascii_digit).collect();
			if digits.len() < 2 {
				return None;
			}
			let oh: i64 = digits[..2].parse().ok()?;
			let om: i64 = digits.get(2..4).and_then(|d| d.parse().ok()).unwrap_or(0);
			Some(wall - sign * (oh * 3600 + om * 60))
		}
		None if fallback_zone.eq_ignore_ascii_case("utc") || fallback_zone.is_empty() => Some(wall),
		None => Some(zone_ts(fallback_zone, date, h * 60 + m) + sec as i64),
	}
}

/// `2026-10-22T08:00:00Z`.
pub fn rfc3339_utc(ts: i64) -> String {
	let date = Date::from_days(ts.div_euclid(86_400));
	let secs = ts.rem_euclid(86_400);
	format!(
		"{}T{:02}:{:02}:{:02}Z",
		date.iso(),
		secs / 3600,
		secs % 3600 / 60,
		secs % 60
	)
}

/// `20261022T080000Z`, iCalendar's UTC form.
pub fn ical_utc(ts: i64) -> String {
	rfc3339_utc(ts).replace(['-', ':'], "")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn civil_dates_round_trip() {
		for (y, m, d) in [
			(1970, 1, 1),
			(2000, 2, 29),
			(2026, 10, 22),
			(1969, 12, 31),
			(2100, 3, 1),
		] {
			let date = Date::new(y, m, d);
			assert_eq!(Date::from_days(date.days()), date);
		}
		assert_eq!(Date::new(1970, 1, 1).days(), 0);
		assert_eq!(Date::new(2026, 10, 22).weekday(), 4, "a Thursday");
		assert_eq!(Date::new(2026, 1, 31).add_months(1), Date::new(2026, 2, 28));
		assert_eq!(Date::new(2026, 11, 15).add_months(3), Date::new(2027, 2, 15));
		assert_eq!(Date::parse("20261022"), Some(Date::new(2026, 10, 22)));
		assert_eq!(Date::parse("2026-02-30"), None);
	}

	#[test]
	fn rfc3339_handles_offsets_fractions_and_zones() {
		let utc = 1_792_656_000; // 2026-10-22T08:00:00Z
		assert_eq!(parse_rfc3339("2026-10-22T08:00:00Z", ""), Some(utc));
		assert_eq!(parse_rfc3339("2026-10-22T10:00:00+02:00", ""), Some(utc));
		assert_eq!(parse_rfc3339("2026-10-22T03:30:00-04:30", ""), Some(utc));
		assert_eq!(parse_rfc3339("2026-10-22T08:00:00.0000000", "UTC"), Some(utc));
		assert_eq!(
			parse_rfc3339("2026-10-22T10:00:00", "Europe/Berlin"),
			Some(utc),
			"CEST is UTC+2"
		);
		assert_eq!(
			parse_rfc3339("2026-12-22T09:00:00", "Europe/Berlin"),
			Some(utc + 61 * 86_400),
			"CET is UTC+1"
		);
		assert_eq!(rfc3339_utc(utc), "2026-10-22T08:00:00Z");
		assert_eq!(ical_utc(utc), "20261022T080000Z");
	}

	#[test]
	fn zone_times_cross_dst() {
		// New York: EDT (UTC-4) until 1 Nov 2026, then EST (UTC-5).
		let d = Date::new(2026, 10, 31);
		assert_eq!(zone_ts("America/New_York", d, 9 * 60), d.days() * 86_400 + 13 * 3600);
		let d = Date::new(2026, 11, 2);
		assert_eq!(zone_ts("America/New_York", d, 9 * 60), d.days() * 86_400 + 14 * 3600);
	}

	#[test]
	fn local_time_round_trips() {
		let ts = local_ts(Date::new(2026, 10, 22), 9 * 60 + 30);
		assert_eq!(local(ts), (Date::new(2026, 10, 22), 9 * 60 + 30));
		assert!(!local_zone().is_empty());
	}
}
