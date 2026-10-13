//! Per-workspace notes: one plain markdown file per workspace, which nothing
//! prunes — lines stay until you delete them. `- [ ]` lines are the tasks.
//! Captures from the page land under a `## YYYY-MM-DD` heading for the day.
//! The file is the source of truth, so any editor can change it; the panel
//! (`notes_html.rs`) re-reads it when it takes focus.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::Deserialize;

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
    let title = if title.trim().is_empty() {
        url
    } else {
        title.trim()
    };
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
    format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    )
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
        assert_eq!(
            day,
            "## 2026-10-13\n\n- [ ] a\n- [ ] b\n\n> q\n\n— [t](u)\n"
        );
        let next = append(
            "my notes\n\n## 2026-10-13\n\n- [x] a\n\n",
            "2026-10-14",
            "- [ ] c",
        );
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
}
