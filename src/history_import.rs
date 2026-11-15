//! History from another browser on this Mac, so the palette finds the user's
//! sites on the first day instead of after weeks of browsing.
//!
//! Every browser keeps its history in a SQLite file that it holds locked
//! while it runs. The file and its write-ahead log are copied to a temp
//! directory and read there, so the other browser's data is never opened in
//! place, let alone written.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::browser::HistoryEntry;

const MICROS_PER_SEC: f64 = 1_000_000.0;
/// Seconds from 1601-01-01 (Chromium's epoch) to 1970-01-01.
const CHROMIUM_EPOCH_OFFSET_SECS: f64 = 11_644_473_600.0;
/// Seconds from 1970-01-01 to 2001-01-01 (Safari's epoch).
const SAFARI_EPOCH_OFFSET_SECS: f64 = 978_307_200.0;

const SAFARI_ID: &str = "safari";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
	Chromium,
	Firefox,
	Safari,
}

impl Format {
	/// One row per web page, newest first: URL, title, visit count, last
	/// visit in the browser's own clock. `?1` caps the rows, so the browser's
	/// own pages (`chrome://…`, `place:…`) are left out here, not afterwards.
	fn query(self) -> &'static str {
		match self {
			Format::Chromium => {
				"SELECT url, title, visit_count, last_visit_time FROM urls \
				 WHERE hidden = 0 AND last_visit_time > 0 \
				 AND (url LIKE 'https://%' OR url LIKE 'http://%') \
				 ORDER BY last_visit_time DESC LIMIT ?1"
			}
			Format::Firefox => {
				"SELECT url, title, visit_count, last_visit_date FROM moz_places \
				 WHERE hidden = 0 AND visit_count > 0 AND last_visit_date IS NOT NULL \
				 AND (url LIKE 'https://%' OR url LIKE 'http://%') \
				 ORDER BY last_visit_date DESC LIMIT ?1"
			}
			// With a bare MAX(), SQLite takes the other columns from the row
			// that holds the maximum: the title of the latest visit.
			Format::Safari => {
				"SELECT i.url, v.title, i.visit_count, MAX(v.visit_time) FROM history_items i \
				 JOIN history_visits v ON v.history_item = i.id \
				 WHERE i.url LIKE 'https://%' OR i.url LIKE 'http://%' \
				 GROUP BY i.id ORDER BY 4 DESC LIMIT ?1"
			}
		}
	}

	fn unix_secs(self, last_visit: f64) -> u64 {
		let secs = match self {
			Format::Chromium => last_visit / MICROS_PER_SEC - CHROMIUM_EPOCH_OFFSET_SECS,
			Format::Firefox => last_visit / MICROS_PER_SEC,
			Format::Safari => last_visit + SAFARI_EPOCH_OFFSET_SECS,
		};
		secs.max(0.0) as u64
	}
}

/// `(id, name, profiles directory under ~/Library/Application Support,
/// history file in a profile, format)`.
const BROWSERS: &[(&str, &str, &str, &str, Format)] = &[
	("arc", "Arc", "Arc/User Data", "History", Format::Chromium),
	("chrome", "Chrome", "Google/Chrome", "History", Format::Chromium),
	(
		"brave",
		"Brave",
		"BraveSoftware/Brave-Browser",
		"History",
		Format::Chromium,
	),
	("edge", "Edge", "Microsoft Edge", "History", Format::Chromium),
	("vivaldi", "Vivaldi", "Vivaldi", "History", Format::Chromium),
	(
		"firefox",
		"Firefox",
		"Firefox/Profiles",
		"places.sqlite",
		Format::Firefox,
	),
];

/// A browser whose history can be imported.
pub struct Source {
	pub id: &'static str,
	pub name: &'static str,
	db: PathBuf,
	format: Format,
}

/// Browsers with a history file on this Mac. A browser with several profiles
/// offers the one used last.
///
/// Safari is always listed: macOS hides its folder from apps without Full
/// Disk Access, so the file can't be seen until the user tries to read it.
pub fn sources() -> Vec<Source> {
	let Some(home) = dirs::home_dir() else {
		return Vec::new();
	};
	let support = home.join("Library/Application Support");
	let mut found: Vec<Source> = BROWSERS
		.iter()
		.filter_map(|&(id, name, profiles, file, format)| {
			let db = latest_profile_db(&support.join(profiles), file)?;
			Some(Source { id, name, db, format })
		})
		.collect();
	found.push(Source {
		id: SAFARI_ID,
		name: "Safari",
		db: home.join("Library/Safari/History.db"),
		format: Format::Safari,
	});
	found
}

/// Script that fills the settings panel's browser picker.
pub fn settings_script() -> String {
	let browsers: Vec<serde_json::Value> = sources()
		.iter()
		.map(|s| serde_json::json!({ "id": s.id, "name": s.name }))
		.collect();
	format!(
		"window.__setImportSources && window.__setImportSources({})",
		serde_json::Value::Array(browsers)
	)
}

/// The `file` of the profile under `profiles` that was written last.
fn latest_profile_db(profiles: &Path, file: &str) -> Option<PathBuf> {
	std::fs::read_dir(profiles)
		.ok()?
		.flatten()
		.map(|profile| profile.path().join(file))
		.filter_map(|db| {
			let modified = db.metadata().ok()?.modified().ok()?;
			Some((modified, db))
		})
		.max_by_key(|(modified, _)| *modified)
		.map(|(_, db)| db)
}

/// The `limit` most recently visited pages of the browser `id`, newest first.
/// The error is a sentence for the user.
pub fn read(id: &str, limit: usize) -> Result<Vec<HistoryEntry>, String> {
	let source = sources()
		.into_iter()
		.find(|s| s.id == id)
		.ok_or_else(|| format!("No history found for \"{id}\"."))?;
	let copy = Snapshot::take(&source.db).map_err(|e| {
		if source.id == SAFARI_ID && e.kind() == std::io::ErrorKind::PermissionDenied {
			"macOS protects Safari's history. Allow Octoweb in System Settings → Privacy & Security → \
			 Full Disk Access, then import again."
				.to_string()
		} else {
			format!("Could not read {}'s history: {e}", source.name)
		}
	})?;
	read_db(&copy.db, source.format, limit).map_err(|e| format!("Could not read {}'s history: {e}", source.name))
}

fn read_db(db: &Path, format: Format, limit: usize) -> rusqlite::Result<Vec<HistoryEntry>> {
	let conn = Connection::open(db)?;
	let mut stmt = conn.prepare(format.query())?;
	let rows = stmt.query_map([limit as i64], |row| {
		Ok((
			row.get::<_, String>(0)?,
			row.get::<_, Option<String>>(1)?,
			row.get::<_, i64>(2)?,
			row.get::<_, f64>(3)?,
		))
	})?;
	let mut entries = Vec::new();
	for row in rows {
		let (url, title, visits, last_visit) = row?;
		entries.push(HistoryEntry {
			title: title.unwrap_or_default(),
			url,
			visited_at: format.unix_secs(last_visit),
			visit_count: visits.clamp(1, i64::from(u32::MAX)) as u32,
		});
	}
	Ok(entries)
}

/// A private copy of a history database, removed on drop.
struct Snapshot {
	dir: PathBuf,
	db: PathBuf,
}

impl Snapshot {
	fn take(db: &Path) -> std::io::Result<Self> {
		static NEXT: AtomicU64 = AtomicU64::new(0);
		let dir = std::env::temp_dir().join(format!(
			"octoweb-history-import-{}-{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed)
		));
		// A run that crashed under the same pid may have left one behind.
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir)?;
		let snapshot = Snapshot {
			db: dir.join("history.db"),
			dir,
		};
		// A running browser keeps its latest pages in the write-ahead log;
		// SQLite replays the copy of it when the copied database is opened.
		// The log goes first: if the browser folds it into the database
		// between the two copies, the database copy already holds those
		// pages, where the other order would pair an old database with a
		// log that no longer matches it.
		let wal = wal_path(db);
		if wal.exists() {
			std::fs::copy(wal, wal_path(&snapshot.db))?;
		}
		std::fs::copy(db, &snapshot.db)?;
		Ok(snapshot)
	}
}

impl Drop for Snapshot {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.dir);
	}
}

fn wal_path(db: &Path) -> PathBuf {
	let mut name = db.as_os_str().to_owned();
	name.push("-wal");
	PathBuf::from(name)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A scratch directory with a database built from `setup`.
	fn database(name: &str, setup: &str) -> (PathBuf, PathBuf) {
		let dir = std::env::temp_dir().join(format!("octoweb-history-import-test-{}-{name}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let db = dir.join("source.db");
		Connection::open(&db).unwrap().execute_batch(setup).unwrap();
		(dir, db)
	}

	const CHROMIUM_SETUP: &str = "
		CREATE TABLE urls(id INTEGER PRIMARY KEY AUTOINCREMENT, url LONGVARCHAR, title LONGVARCHAR,
			visit_count INTEGER DEFAULT 0 NOT NULL, typed_count INTEGER DEFAULT 0 NOT NULL,
			last_visit_time INTEGER NOT NULL, hidden INTEGER DEFAULT 0 NOT NULL);
		INSERT INTO urls(url, title, visit_count, last_visit_time, hidden) VALUES
			('https://old.example/', 'Old', 3, 13420724436028331, 0),
			('https://new.example/', 'New', 7, 13429868277631058, 0),
			('https://hidden.example/', 'Hidden', 1, 13429868277631059, 1),
			('chrome://settings/', 'Settings', 2, 13429868277631060, 0),
			('https://never.example/', 'Never', 0, 0, 0);";

	#[test]
	fn chromium_rows_come_newest_first_in_unix_time() {
		let (dir, db) = database("chromium", CHROMIUM_SETUP);
		let entries = read_db(&db, Format::Chromium, 10).unwrap();
		let urls: Vec<&str> = entries.iter().map(|e| e.url.as_str()).collect();
		assert_eq!(urls, ["https://new.example/", "https://old.example/"]);
		assert_eq!(entries[0].title, "New");
		assert_eq!(entries[0].visit_count, 7);
		assert_eq!(entries[0].visited_at, 1_785_394_677);
		let _ = std::fs::remove_dir_all(dir);
	}

	#[test]
	fn limit_keeps_the_most_recent_pages() {
		let (dir, db) = database("limit", CHROMIUM_SETUP);
		let entries = read_db(&db, Format::Chromium, 1).unwrap();
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].url, "https://new.example/");
		let _ = std::fs::remove_dir_all(dir);
	}

	#[test]
	fn firefox_rows_use_microseconds_since_1970() {
		let (dir, db) = database(
			"firefox",
			"CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR,
				visit_count INTEGER DEFAULT 0, hidden INTEGER DEFAULT 0 NOT NULL, last_visit_date INTEGER);
			INSERT INTO moz_places(url, title, visit_count, hidden, last_visit_date) VALUES
				('https://a.example/', NULL, 2, 0, 1700000000000000),
				('https://bookmark-only.example/', 'Never visited', 0, 0, NULL),
				('place:sort=8', 'Query', 1, 0, 1700000001000000);",
		);
		let entries = read_db(&db, Format::Firefox, 10).unwrap();
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].url, "https://a.example/");
		assert_eq!(entries[0].title, "");
		assert_eq!(entries[0].visited_at, 1_700_000_000);
		let _ = std::fs::remove_dir_all(dir);
	}

	#[test]
	fn safari_rows_take_the_latest_visit_and_its_title() {
		let (dir, db) = database(
			"safari",
			"CREATE TABLE history_items(id INTEGER PRIMARY KEY, url TEXT NOT NULL UNIQUE, visit_count INTEGER NOT NULL);
			CREATE TABLE history_visits(id INTEGER PRIMARY KEY, history_item INTEGER NOT NULL,
				visit_time REAL NOT NULL, title TEXT NULL);
			INSERT INTO history_items(id, url, visit_count) VALUES (1, 'https://a.example/', 2);
			INSERT INTO history_visits(history_item, visit_time, title) VALUES
				(1, 700000000.5, 'First title'),
				(1, 721692800.0, 'Latest title');",
		);
		let entries = read_db(&db, Format::Safari, 10).unwrap();
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].title, "Latest title");
		assert_eq!(entries[0].visit_count, 2);
		assert_eq!(entries[0].visited_at, 1_700_000_000);
		let _ = std::fs::remove_dir_all(dir);
	}

	#[test]
	fn snapshot_reads_pages_still_in_the_write_ahead_log() {
		let (dir, db) = database("wal", CHROMIUM_SETUP);
		// A second connection left open keeps its write in the log, the way
		// a running browser does.
		let writer = Connection::open(&db).unwrap();
		writer
			.execute_batch(
				"PRAGMA journal_mode = WAL;
				 INSERT INTO urls(url, title, visit_count, last_visit_time) VALUES
					('https://latest.example/', 'Latest', 1, 13429868277631099);",
			)
			.unwrap();
		let copy = Snapshot::take(&db).unwrap();
		let entries = read_db(&copy.db, Format::Chromium, 10).unwrap();
		assert_eq!(entries[0].url, "https://latest.example/");
		let copy_dir = copy.dir.clone();
		drop(copy);
		assert!(!copy_dir.exists());
		drop(writer);
		let _ = std::fs::remove_dir_all(dir);
	}
}
