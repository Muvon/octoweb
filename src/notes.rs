//! Per-workspace notes: one plain markdown file per workspace, which nothing
//! prunes — lines stay until you delete them. `- [ ]` lines are the tasks.
//! Captures from the page land under a `## YYYY-MM-DD` heading for the day.
//! The file is the source of truth, so any editor can change it; the panel
//! (`notes_html.rs`) re-reads it when it takes focus.
//!
//! Tasks (spec `specs/003-tasks`) are read from the same file: a due date is
//! Obsidian Tasks' `📅 YYYY-MM-DD`, and `###` headings are meeting notes.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::calendar_time::Date;

/// Messages from the notes panel page.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
	/// Workspace `ws`'s buffer, after typing pauses or the panel loses focus.
	Save {
		ws: String,
		text: Text,
	},
	/// ⌘-click on a link.
	OpenUrl {
		url: String,
	},
	Reveal,
	Hide,
	Fullscreen,
	/// Switch the pane to `mail` (spec PANE-3).
	Tab {
		tab: String,
	},
	/// A task linking a mail conversation was ticked or unticked (TRI-10).
	Task {
		url: String,
		done: bool,
	},
	/// The date a task line's `>` words name (TSK-3), answered with
	/// `window.__notesDue(id, iso | null)`.
	Due {
		id: u64,
		phrase: String,
	},
	/// The panel switched between the text and the Tasks view.
	View {
		view: String,
	},
	/// The panel took the keyboard.
	Focus,
	/// The panel lost the keyboard.
	Blur,
	/// Live column width while its left edge is dragged, in logical points.
	Resize {
		width: u32,
	},
	/// Final column width, to persist.
	ResizeEnd {
		width: u32,
	},
	ResizeReset,
}

/// A notes buffer. Its Debug output is redacted: notes hold whatever the user
/// keeps there, secrets included.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Text(pub String);

impl std::fmt::Debug for Text {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Text(..)")
	}
}

pub fn path(ws_id: &str) -> PathBuf {
	crate::config::base_dir()
		.join("octoweb")
		.join("notes")
		.join(format!("{ws_id}.md"))
}

/// Workspace `ws_id`'s notes; empty before the first save. Any other read
/// failure is an error, so a blank panel never gets saved over the file.
pub fn load(ws_id: &str) -> io::Result<String> {
	match fs::read_to_string(path(ws_id)) {
		Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
		read => read,
	}
}

pub fn save(ws_id: &str, text: &str) -> io::Result<()> {
	let path = path(ws_id);
	fs::create_dir_all(path.parent().expect("notes path has a parent"))?;
	let tmp = path.with_extension("md.tmp");
	fs::write(&tmp, text)?;
	fs::rename(&tmp, &path)
}

/// What the capture chord adds for a page: the selection quoted over a link
/// to the page, or with nothing selected a task linking it.
pub fn capture_entry(title: &str, url: &str, selection: &str) -> String {
	let title: String = title.chars().filter(|c| !matches!(c, '[' | ']')).collect();
	let title = if title.trim().is_empty() { url } else { title.trim() };
	let link = format!("[{title}]({})", url.replace('(', "%28").replace(')', "%29"));
	if selection.is_empty() {
		return format!("- [ ] {link}");
	}
	let quote: Vec<String> = selection
		.lines()
		.map(|line| match line.trim_end() {
			"" => ">".to_string(),
			line => format!("> {line}"),
		})
		.collect();
	format!("{}\n\n— {link}", quote.join("\n"))
}

/// `text` with `entry` appended under the `## date` heading, which is added
/// unless it is already the last heading. Consecutive list items stay one
/// list; anything else is its own paragraph.
pub fn append(text: &str, date: &str, entry: &str) -> String {
	let heading = format!("## {date}");
	let mut out = text.trim_end().to_string();
	let last_heading = out.lines().rev().find(|line| line.starts_with("## "));
	if last_heading.map(str::trim_end) != Some(heading.as_str()) {
		if !out.is_empty() {
			out.push_str("\n\n");
		}
		out.push_str(&heading);
	}
	let last_line = out.lines().last().unwrap_or("");
	let separator = if last_line.starts_with("- ") && entry.starts_with("- ") {
		"\n"
	} else {
		"\n\n"
	};
	out.push_str(separator);
	out.push_str(entry);
	out.push('\n');
	out
}

/// `text` without the first line that is exactly `entry`, or `None` when no
/// line is — undoing a capture must not touch anything the user wrote.
pub fn remove_entry(text: &str, entry: &str) -> Option<String> {
	let lines: Vec<&str> = text.lines().collect();
	let at = lines.iter().position(|line| *line == entry)?;
	let mut kept: Vec<&str> = lines[..at].to_vec();
	kept.extend_from_slice(&lines[at + 1..]);
	let mut out = kept.join("\n");
	if text.ends_with('\n') {
		out.push('\n');
	}
	Some(out)
}

/// Local broken-down time of `ts` (Unix seconds).
fn local_tm(ts: i64) -> libc::tm {
	// SAFETY: localtime_r fills the zeroed `tm` on this stack from `ts`.
	unsafe {
		let mut tm: libc::tm = std::mem::zeroed();
		libc::localtime_r(&(ts as libc::time_t), &mut tm);
		tm
	}
}

/// The local hour, 0–23.
pub fn local_hour() -> i32 {
	// SAFETY: time(NULL) only reads the clock.
	local_tm(unsafe { libc::time(std::ptr::null_mut()) }).tm_hour
}

/// Today's local date as `YYYY-MM-DD`.
pub fn today() -> String {
	// SAFETY: time(NULL) only reads the clock.
	let tm = local_tm(unsafe { libc::time(std::ptr::null_mut()) });
	format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

/// `Tue, 13 Oct 2026 at 10:00` in local time, for quoted mail.
pub fn format_time(ts: i64) -> String {
	const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
	const MONTHS: [&str; 12] = [
		"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
	];
	let tm = local_tm(ts);
	format!(
		"{}, {} {} {} at {:02}:{:02}",
		DAYS[tm.tm_wday as usize],
		tm.tm_mday,
		MONTHS[tm.tm_mon as usize],
		tm.tm_year + 1900,
		tm.tm_hour,
		tm.tm_min
	)
}

// ── Tasks (spec 003-tasks) ───────────────────────────────────────────────────────────

/// Obsidian Tasks' due-date signifier (TSK-2).
pub const DUE: &str = "\u{1F4C5}";

/// One task line of a notes file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Task {
	/// Zero-based line number in the file.
	pub line: usize,
	/// The whole line as written: how a change finds it again.
	pub raw: String,
	/// What follows the box, without its due date.
	pub text: String,
	pub done: bool,
	/// `YYYY-MM-DD`, from the first `📅 YYYY-MM-DD` in the line.
	pub due: Option<String>,
	/// The `## YYYY-MM-DD` heading above it: the day it was captured.
	pub day: Option<String>,
	/// The mail conversation it links (`octoweb-mail://…`), if any.
	pub mail: Option<String>,
}

/// Where the box's mark is in a task line, and whether it is ticked:
/// `- [ ] text` or `* [x] text` at any indent (TSK-1), as the panel's `TASK`
/// pattern reads them.
fn task_box(line: &str) -> Option<(usize, bool)> {
	let indent = line.len() - line.trim_start().len();
	let rest = &line[indent..];
	let rest = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* "))?;
	let mark = rest.strip_prefix('[')?.chars().next()?;
	if !matches!(mark, ' ' | 'x' | 'X') || rest.get(2..3) != Some("]") {
		return None;
	}
	Some((indent + 3, mark != ' '))
}

/// The first `📅 YYYY-MM-DD` in `line`: its date and the byte range it spans.
fn due_field(line: &str) -> Option<(Date, std::ops::Range<usize>)> {
	let mut from = 0;
	while let Some(at) = line[from..].find(DUE).map(|i| i + from) {
		let after = &line[at + DUE.len()..];
		let gap = after.len() - after.trim_start().len();
		let start = at + DUE.len() + gap;
		if let Some(date) = line.get(start..start + 10).and_then(Date::parse) {
			return Some((date, at..start + 10));
		}
		from = at + DUE.len();
	}
	None
}

/// `YYYY-MM-DD` of a `## YYYY-MM-DD` day heading.
fn day_heading(line: &str) -> Option<Option<String>> {
	let rest = line.strip_prefix("## ")?;
	Some(
		Date::parse(rest.trim())
			.filter(|_| rest.trim().len() == 10)
			.map(Date::iso),
	)
}

/// Every task line of `text`, in file order.
pub fn tasks(text: &str) -> Vec<Task> {
	let mut out = Vec::new();
	let mut day: Option<String> = None;
	for (line, raw) in text.split('\n').enumerate() {
		let raw = raw.strip_suffix('\r').unwrap_or(raw);
		if let Some(heading) = day_heading(raw) {
			day = heading;
			continue;
		}
		let Some((mark, done)) = task_box(raw) else {
			continue;
		};
		let body = &raw[mark + 2..];
		let due = due_field(body);
		let text = match &due {
			Some((_, range)) => format!("{} {}", &body[..range.start], &body[range.end..]),
			None => body.to_string(),
		};
		let mail = raw.find("octoweb-mail://").map(|at| {
			let url = &raw[at..];
			url[..url.find([')', ' ', '\t']).unwrap_or(url.len())].to_string()
		});
		out.push(Task {
			line,
			raw: raw.to_string(),
			text: text.split_whitespace().collect::<Vec<_>>().join(" "),
			done,
			due: due.map(|(d, _)| d.iso()),
			day: day.clone(),
			mail,
		});
	}
	out
}

/// Open tasks due on or before `today`: the count on the Notes tab (CNT-1).
pub fn due_count(text: &str, today: Date) -> usize {
	let today = today.iso();
	tasks(text)
		.iter()
		.filter(|t| !t.done && t.due.as_ref().is_some_and(|d| *d <= today))
		.count()
}

/// The day `phrase` names, read the way quick add reads dates (TSK-3):
/// `today`, `fri`, `next mon`, `oct 28`, `28/10`, `2026-10-28`, `in 3 days`.
/// `None` unless every word is part of one date, with no time, guest, place
/// or repeat, so a `>` that is not a date stays as typed.
pub fn due_date(phrase: &str, today: Date) -> Option<Date> {
	let phrase = phrase.trim();
	if phrase.is_empty() || phrase.contains('@') || phrase.contains('+') {
		return None;
	}
	let q = crate::calendar_quick::parse(phrase, today, 0);
	(q.title.trim().is_empty()
		&& q.all_day
		&& q.recur.is_empty()
		&& q.attendees.is_empty()
		&& q.location.is_empty()
		&& !q.video
		&& q.end_date == q.date)
		.then_some(q.date)
}

/// The date the first words of `phrase` name, and the words after it:
/// `today call bank` is today and `call bank` (TSK-3), so the date can come
/// first or last. `None` when no leading words are a date.
pub fn due_split(phrase: &str, today: Date) -> Option<(Date, String)> {
	let words: Vec<&str> = phrase.split_whitespace().collect();
	(1..=words.len().min(4))
		.rev()
		.find_map(|n| due_date(&words[..n].join(" "), today).map(|d| (d, words[n..].join(" "))))
}

/// A new open task line for `text`, due on `due` if given. Newlines become
/// spaces and brackets go (AGT-2), so a task cannot turn into a link that
/// hides where it goes.
pub fn task_line(text: &str, due: Option<Date>) -> String {
	let mut text: String = text.replace(['\n', '\r'], " ").replace(['[', ']'], "");
	// One date per task: a `📅` already in the text would win over `due`.
	while let Some((_, range)) = due.and(due_field(&text)) {
		text.replace_range(range, " ");
	}
	let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
	match due {
		Some(d) => format!("- [ ] {text} {DUE} {}", d.iso()),
		None => format!("- [ ] {text}"),
	}
}

/// A task line from one task in an AI answer: a trailing ` >YYYY-MM-DD`
/// (or any `>` date) becomes its `📅` date (calendar AI-4).
pub fn answer_task(answer: &str, today: Date) -> String {
	match answer
		.rsplit_once(" >")
		.and_then(|(text, phrase)| Some((text, due_date(phrase, today)?)))
	{
		Some((text, due)) => task_line(text, Some(due)),
		None => task_line(answer, None),
	}
}

/// `text` with the task written as `raw` ticked (or unticked): the line at
/// `line` when it still reads `raw`, else the nearest line that does (the
/// same task can repeat under several days). `None` when no line is that
/// task any more — the file changed under the caller.
pub fn set_done(text: &str, line: usize, raw: &str, done: bool) -> Option<String> {
	let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
	let same = |l: &String| l.strip_suffix('\r').unwrap_or(l) == raw;
	let at = (0..lines.len())
		.filter(|&i| same(&lines[i]))
		.min_by_key(|i| i.abs_diff(line))?;
	let (mark, _) = task_box(&lines[at])?;
	lines[at].replace_range(mark..mark + 1, if done { "x" } else { " " });
	Some(lines.join("\n"))
}

/// A `###` section of the notes: meeting notes are `### HH:MM Title` under
/// the meeting's day (calendar EVT-4).
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
	/// The `## YYYY-MM-DD` heading above it.
	pub day: Option<String>,
	/// The heading without `### ` and its leading time.
	pub title: String,
	/// Line of the heading.
	pub start: usize,
	/// Line after the section's last: the next heading of level 1 to 3, or
	/// the end of the file.
	pub end: usize,
	/// What is under the heading, trimmed.
	pub body: String,
}

/// `9:30` or `09:30`, as a meeting note heading starts.
fn is_clock(word: &str) -> bool {
	let Some((h, m)) = word.split_once(':') else {
		return false;
	};
	(1..=2).contains(&h.len()) && m.len() == 2 && h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit())
}

/// Every `###` section of `text`, in file order.
pub fn sections(text: &str) -> Vec<Section> {
	let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
	let ends_section = |l: &str| l.starts_with("# ") || l.starts_with("## ") || l.starts_with("### ");
	let mut out = Vec::new();
	let mut day: Option<String> = None;
	for (i, line) in lines.iter().enumerate() {
		if let Some(heading) = day_heading(line) {
			day = heading;
			continue;
		}
		if line.starts_with("# ") {
			day = None;
			continue;
		}
		let Some(heading) = line.strip_prefix("### ") else {
			continue;
		};
		let heading = heading.trim();
		let title = match heading.split_once(' ') {
			Some((first, rest)) if is_clock(first) => rest.trim(),
			_ => heading,
		};
		let end = (i + 1..lines.len())
			.find(|&j| ends_section(lines[j]))
			.unwrap_or(lines.len());
		out.push(Section {
			day: day.clone(),
			title: title.to_string(),
			start: i,
			end,
			body: lines[i + 1..end].join("\n").trim().to_string(),
		});
	}
	out
}

/// The notes section of the meeting titled `title` on day `day`
/// (`YYYY-MM-DD`): the last such heading under that day.
pub fn meeting_section<'a>(sections: &'a [Section], title: &str, day: &str) -> Option<&'a Section> {
	sections
		.iter()
		.rfind(|s| s.day.as_deref() == Some(day) && s.title.eq_ignore_ascii_case(title.trim()))
}

/// The notes of the last meeting titled `title` before day `day`: what the
/// previous occurrence left (calendar AI-3).
pub fn previous_meeting<'a>(sections: &'a [Section], title: &str, day: &str) -> Option<&'a Section> {
	sections
		.iter()
		.filter(|s| s.day.as_deref().is_some_and(|d| d < day) && s.title.eq_ignore_ascii_case(title.trim()))
		.max_by(|a, b| a.day.cmp(&b.day).then(a.start.cmp(&b.start)))
}

/// `text` with `entries` added at the end of the section whose heading is
/// line `start` and reads `heading`, or, when lines moved, the last heading
/// reading so under day `day` (a repeating meeting repeats its heading every
/// week); an empty `- ` bullet the meeting note left there is replaced.
/// `None` when that heading is gone.
pub fn add_to_section(text: &str, start: usize, heading: &str, day: &str, entries: &[String]) -> Option<String> {
	let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
	let all = sections(text);
	let reads = |s: &&Section| lines[s.start].strip_suffix('\r').unwrap_or(&lines[s.start]) == heading;
	let section = all
		.iter()
		.find(|s| s.start == start && reads(s))
		.or_else(|| all.iter().filter(reads).rfind(|s| s.day.as_deref() == Some(day)))?
		.clone();
	let start = section.start;
	let last = (start + 1..section.end)
		.rev()
		.find(|&i| !lines[i].trim().is_empty())
		.unwrap_or(start);
	let mut at = last + 1;
	if last > start && lines[last].trim() == "-" {
		lines.remove(last);
		at = last;
	} else if last == start {
		// A bare heading: a blank line before the list, as markdown reads it.
		lines.insert(at, String::new());
		at += 1;
	}
	for (i, entry) in entries.iter().enumerate() {
		lines.insert(at + i, entry.clone());
	}
	Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn capture_is_a_task_or_a_quote_with_a_safe_link() {
		assert_eq!(
			capture_entry("Docs [beta]", "https://e.x/a_(b)", ""),
			"- [ ] [Docs beta](https://e.x/a_%28b%29)"
		);
		assert_eq!(
			capture_entry("", "https://e.x", "one\n\ntwo  "),
			"> one\n>\n> two\n\n— [https://e.x](https://e.x)"
		);
	}

	#[test]
	fn append_opens_a_day_once_and_keeps_tasks_in_one_list() {
		let day = append("", "2026-10-13", "- [ ] a");
		assert_eq!(day, "## 2026-10-13\n\n- [ ] a\n");
		let day = append(&day, "2026-10-13", "- [ ] b");
		assert_eq!(day, "## 2026-10-13\n\n- [ ] a\n- [ ] b\n");
		let day = append(&day, "2026-10-13", "> q\n\n— [t](u)");
		assert_eq!(day, "## 2026-10-13\n\n- [ ] a\n- [ ] b\n\n> q\n\n— [t](u)\n");
		let next = append("my notes\n\n## 2026-10-13\n\n- [x] a\n\n", "2026-10-14", "- [ ] c");
		assert_eq!(
			next,
			"my notes\n\n## 2026-10-13\n\n- [x] a\n\n## 2026-10-14\n\n- [ ] c\n"
		);
	}

	#[test]
	fn today_is_an_iso_date() {
		let today = today();
		assert_eq!(today.len(), 10);
		assert_eq!(today.as_bytes()[4], b'-');
		assert_eq!(today.as_bytes()[7], b'-');
	}

	// Thursday 22 October 2026, as quick add's tests.
	const THU: Date = Date { y: 2026, m: 10, d: 22 };

	#[test]
	fn tasks_carry_their_due_date_day_and_mail_link() {
		let text = "top\n- [ ] loose\n\n## 2026-10-21\n\n- [ ] Pay rent \u{1F4C5} 2026-10-23 now\n  * [x] done\n\
		            - [ ] [Contract — Anna](octoweb-mail://ab/t%40x) \u{1F4C5}2026-10-22\n- [] not a task\n\
		            ## Ideas\n- [X] later\n";
		let t = tasks(text);
		assert_eq!(t.len(), 5);
		assert_eq!(
			(t[0].line, t[0].text.as_str(), t[0].day.as_deref(), t[0].due.as_deref()),
			(1, "loose", None, None)
		);
		assert_eq!(t[1].text, "Pay rent now");
		assert_eq!(
			(t[1].due.as_deref(), t[1].day.as_deref()),
			(Some("2026-10-23"), Some("2026-10-21"))
		);
		assert!(t[2].done && t[2].text == "done");
		assert_eq!(t[3].mail.as_deref(), Some("octoweb-mail://ab/t%40x"));
		assert_eq!(t[3].due.as_deref(), Some("2026-10-22"));
		// A heading that is not a day ends the day.
		assert!(t[4].done && t[4].day.is_none());
		assert_eq!(due_count(text, THU), 1);
		assert_eq!(due_count(text, THU.add_days(1)), 2);
	}

	#[test]
	fn due_words_are_dates_or_nothing() {
		let d = |s: &str| due_date(s, THU).map(Date::iso);
		assert_eq!(d("tomorrow").as_deref(), Some("2026-10-23"));
		assert_eq!(d("fri").as_deref(), Some("2026-10-23"));
		assert_eq!(d("oct 28").as_deref(), Some("2026-10-28"));
		assert_eq!(d("28/10").as_deref(), Some("2026-10-28"));
		assert_eq!(d("2026-11-02").as_deref(), Some("2026-11-02"));
		assert_eq!(d("in 3 days").as_deref(), Some("2026-10-25"));
		for no in ["", "call bob", "3pm", "fri 3pm", "every fri", "fri @home", "oct 24-26"] {
			assert_eq!(d(no), None, "{no}");
		}
		let s = |p: &str| due_split(p, THU).map(|(d, rest)| (d.iso(), rest));
		assert_eq!(s("today hell"), Some(("2026-10-22".into(), "hell".into())));
		assert_eq!(
			s("next mon call bank"),
			Some((due_date("next mon", THU).unwrap().iso(), "call bank".into()))
		);
		assert_eq!(s("oct 28"), Some(("2026-10-28".into(), String::new())));
		assert_eq!(s("hello there"), None);
	}

	#[test]
	fn new_task_lines_are_plain_and_dated() {
		assert_eq!(task_line("Pay [rent]\nnow", None), "- [ ] Pay rent now");
		assert_eq!(task_line("Pay", Some(THU)), "- [ ] Pay \u{1F4C5} 2026-10-22");
		assert_eq!(
			answer_task("Send deck >2026-10-30", THU),
			"- [ ] Send deck \u{1F4C5} 2026-10-30"
		);
		assert_eq!(answer_task("Compare a >b", THU), "- [ ] Compare a >b");
		assert_eq!(
			task_line("Pay \u{1F4C5} 2026-01-01", Some(THU)),
			"- [ ] Pay \u{1F4C5} 2026-10-22"
		);
	}

	#[test]
	fn ticking_finds_the_task_even_when_it_moved() {
		let text = "a\n- [ ] one\n- [ ] two\n";
		assert_eq!(
			set_done(text, 2, "- [ ] two", true).unwrap(),
			"a\n- [ ] one\n- [x] two\n"
		);
		assert_eq!(
			set_done(text, 0, "- [ ] two", true).unwrap(),
			"a\n- [ ] one\n- [x] two\n"
		);
		assert_eq!(set_done("  * [x] z", 0, "  * [x] z", false).unwrap(), "  * [ ] z");
		assert_eq!(set_done(text, 1, "- [ ] gone", true), None);
		// The same task on two days: the one nearest where it was.
		let twice = "- [ ] call\nx\ny\n- [ ] call\n";
		assert_eq!(
			set_done(twice, 4, "- [ ] call", true).unwrap(),
			"- [ ] call\nx\ny\n- [x] call\n"
		);
	}

	#[test]
	fn meeting_sections_are_found_by_title_and_day() {
		let text = "## 2026-10-15\n\n### 10:00 Weekly sync\n\nWith Anna\n\n- ship it\n\n\
		            ## 2026-10-22\n\n### 9:30 Weekly sync\n\nWith Anna\n\n- \n\n### Other\nx\n";
		let s = sections(text);
		assert_eq!(s.len(), 3);
		assert_eq!(
			(s[0].title.as_str(), s[0].day.as_deref()),
			("Weekly sync", Some("2026-10-15"))
		);
		assert_eq!(s[0].body, "With Anna\n\n- ship it");
		let now = meeting_section(&s, "weekly sync", "2026-10-22").unwrap();
		assert_eq!(now.start, 10);
		assert_eq!(previous_meeting(&s, "Weekly sync", "2026-10-22").unwrap().start, 2);
		assert!(previous_meeting(&s, "Weekly sync", "2026-10-15").is_none());
		// Tasks replace the empty bullet the meeting note left.
		let tasks = ["- [ ] a".to_string(), "- [ ] b".to_string()];
		let added = add_to_section(text, 10, "### 9:30 Weekly sync", "2026-10-22", &tasks).unwrap();
		assert!(added.contains("With Anna\n\n- [ ] a\n- [ ] b\n\n### Other"), "{added}");
		// Lines moved: the heading is found again under its day, not last week's.
		let moved = format!("new line\n{text}");
		let again = add_to_section(&moved, 10, "### 10:00 Weekly sync", "2026-10-15", &tasks).unwrap();
		assert!(
			again.contains("- ship it\n- [ ] a\n- [ ] b\n\n## 2026-10-22"),
			"{again}"
		);
		// Under a bare heading they start after a blank line.
		assert_eq!(
			add_to_section("### X", 0, "### X", "", &["- [ ] a".into()]).unwrap(),
			"### X\n\n- [ ] a"
		);
		assert_eq!(add_to_section(text, 10, "### gone", "2026-10-22", &tasks), None);
	}
}
