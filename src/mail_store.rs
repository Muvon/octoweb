//! Local mail store: one SQLite database with an FTS5 index (spec SYN-3,
//! SYN-4). Holds the synced messages of every account, how they thread, and
//! the state servers cannot keep — pins, snoozes, mutes, sender decisions,
//! reminders, drafts, scheduled sends and server actions waiting for a
//! connection. Nothing here prunes; only the user's actions remove rows.
//!
//! `folder` is where a message is on the server as far as octoweb knows:
//! `inbox`, `sent`, or `archive` for anything that left the inbox.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::mail_parse::{normalize_subject, Addr, Attachment, Parsed};

/// Replies with no usable references join a thread with the same subject and
/// a shared participant only within this window.
const SUBJECT_THREAD_WINDOW_SECS: i64 = 30 * 24 * 3600;

pub const INBOX: &str = "inbox";
pub const SENT: &str = "sent";
pub const ARCHIVE: &str = "archive";

/// One account as the views need it.
#[derive(Debug, Clone)]
pub struct AccountRef {
    pub key: String,
    pub address: String,
    pub gatekeeper: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ThreadRow {
    pub account: String,
    pub thread: String,
    pub subject: String,
    /// Latest sender who is not the account itself, else the latest sender.
    pub from_name: String,
    pub from_addr: String,
    pub count: usize,
    pub unread: bool,
    pub date: i64,
    pub preview: String,
    pub attachment: bool,
    pub pinned: bool,
    pub muted: bool,
    pub class: &'static str,
    /// `snooze` or `reminder` when the thread came back on its own.
    pub returned: Option<String>,
    pub remind_at: Option<i64>,
    pub summary: Option<String>,
    /// Sort key: latest activity, or when it came back.
    pub key: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SenderGroup {
    pub name: String,
    pub addr: String,
    pub unread: usize,
    pub threads: Vec<ThreadRow>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct InboxView {
    pub new_senders: Vec<ThreadRow>,
    pub important: Vec<ThreadRow>,
    pub other: Vec<SenderGroup>,
    pub next_snooze: Option<i64>,
    pub unread_important: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageView {
    pub id: i64,
    pub message_id: String,
    pub folder: String,
    pub uid: Option<u32>,
    pub from: Addr,
    pub reply_to: Option<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub date: i64,
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
    pub body_loaded: bool,
    pub seen: bool,
    pub parents: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ThreadState {
    pub pinned: bool,
    pub muted: bool,
    pub snooze_until: Option<i64>,
    pub returned: Option<String>,
    pub returned_at: Option<i64>,
    pub remind_at: Option<i64>,
    pub summary: Option<String>,
}

/// A message to move or flag on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ServerRef {
    pub uid: Option<u32>,
    pub folder: String,
    pub message_id: String,
}

#[derive(Debug, Clone)]
pub struct Outgoing {
    pub id: i64,
    pub account: String,
    pub raw: Vec<u8>,
    pub send_at: i64,
    pub remind_after: Option<i64>,
    pub thread: Option<String>,
    pub message_id: String,
    pub recipients: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QueuedAction {
    pub id: i64,
    pub kind: String,
    pub refs: Vec<ServerRef>,
}

pub struct Store {
    conn: Connection,
}

/// A thread's message, as much as its inbox row needs.
struct RowMessage {
    subject: String,
    from_name: String,
    from_addr: String,
    date: i64,
    preview: String,
    attachments: String,
    bulk: bool,
    seen: bool,
    folder: String,
}

/// What `insert` did with a synced message.
#[derive(Debug, PartialEq, Eq)]
pub struct Inserted {
    pub id: i64,
    pub thread: String,
    pub new: bool,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        let store = Self {
            conn: Connection::open_in_memory().unwrap(),
        };
        store.migrate().unwrap();
        store
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages(
                id INTEGER PRIMARY KEY,
                account TEXT NOT NULL,
                folder TEXT NOT NULL,
                uid INTEGER,
                message_id TEXT NOT NULL,
                thread TEXT NOT NULL,
                parents TEXT NOT NULL,
                subject TEXT NOT NULL,
                nsubject TEXT NOT NULL,
                from_name TEXT NOT NULL,
                from_addr TEXT NOT NULL,
                reply_to TEXT,
                to_json TEXT NOT NULL,
                cc_json TEXT NOT NULL,
                date INTEGER NOT NULL,
                preview TEXT NOT NULL,
                text TEXT,
                html TEXT,
                attachments TEXT NOT NULL,
                body INTEGER NOT NULL,
                bulk INTEGER NOT NULL,
                seen INTEGER NOT NULL,
                UNIQUE(account, message_id)
            );
            CREATE INDEX IF NOT EXISTS messages_thread ON messages(account, thread);
            CREATE INDEX IF NOT EXISTS messages_folder ON messages(account, folder, uid);
            CREATE INDEX IF NOT EXISTS messages_subject ON messages(account, nsubject, date);
            CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
                subject, people, body, tokenize = 'unicode61 remove_diacritics 2'
            );
            CREATE TABLE IF NOT EXISTS threads(
                account TEXT NOT NULL,
                thread TEXT NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                muted INTEGER NOT NULL DEFAULT 0,
                snooze_until INTEGER,
                returned TEXT,
                returned_at INTEGER,
                remind_at INTEGER,
                remind_since INTEGER,
                summary TEXT,
                done TEXT,
                PRIMARY KEY(account, thread)
            );
            CREATE TABLE IF NOT EXISTS senders(
                account TEXT NOT NULL,
                addr TEXT NOT NULL,
                class TEXT,
                gate TEXT,
                images INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(account, addr)
            );
            CREATE TABLE IF NOT EXISTS folders(
                account TEXT NOT NULL,
                folder TEXT NOT NULL,
                uidvalidity INTEGER NOT NULL,
                PRIMARY KEY(account, folder)
            );
            CREATE TABLE IF NOT EXISTS outbox(
                id INTEGER PRIMARY KEY,
                account TEXT NOT NULL,
                raw BLOB NOT NULL,
                send_at INTEGER NOT NULL,
                remind_after INTEGER,
                thread TEXT,
                message_id TEXT NOT NULL,
                recipients TEXT NOT NULL,
                error TEXT
            );
            CREATE TABLE IF NOT EXISTS drafts(
                key TEXT PRIMARY KEY,
                json TEXT NOT NULL,
                updated INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS actions(
                id INTEGER PRIMARY KEY,
                account TEXT NOT NULL,
                kind TEXT NOT NULL,
                refs TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS meta(
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
    }

    // ── Sync ────────────────────────────────────────────────────────────────

    pub fn uidvalidity(&self, account: &str, folder: &str) -> Option<u32> {
        self.conn
            .query_row(
                "SELECT uidvalidity FROM folders WHERE account = ?1 AND folder = ?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()
            .expect("folders query")
    }

    /// A new UIDVALIDITY makes every stored UID of the folder meaningless.
    pub fn set_uidvalidity(&self, account: &str, folder: &str, value: u32) {
        if self.uidvalidity(account, folder) != Some(value) {
            self.conn
                .execute(
                    "UPDATE messages SET uid = NULL WHERE account = ?1 AND folder = ?2",
                    params![account, folder],
                )
                .expect("reset uids");
        }
        self.conn
            .execute(
                "INSERT INTO folders(account, folder, uidvalidity) VALUES(?1, ?2, ?3)
                 ON CONFLICT(account, folder) DO UPDATE SET uidvalidity = excluded.uidvalidity",
                params![account, folder, value],
            )
            .expect("set uidvalidity");
    }

    /// UIDs of `folder` the store knows, with their message ids.
    pub fn uids(&self, account: &str, folder: &str) -> HashMap<u32, String> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT uid, message_id FROM messages
                 WHERE account = ?1 AND folder = ?2 AND uid IS NOT NULL",
            )
            .expect("uids");
        stmt.query_map(params![account, folder], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("uids")
            .map(|r| r.expect("uid row"))
            .collect()
    }

    pub fn date_of_uid(&self, account: &str, folder: &str, uid: u32) -> Option<i64> {
        self.conn
            .query_row(
                "SELECT date FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid],
                |r| r.get(0),
            )
            .optional()
            .expect("date of uid")
    }

    /// Message ids an unfinished server action still has to move; sync must
    /// not put them back where they were.
    pub fn pending_ids(&self, account: &str) -> HashSet<String> {
        self.actions(account)
            .into_iter()
            .flat_map(|a| a.refs.into_iter().map(|r| r.message_id))
            .collect()
    }

    /// Store a synced message, or update where an already stored one is now.
    pub fn insert(
        &self,
        account: &str,
        folder: &str,
        uid: Option<u32>,
        seen: bool,
        body_loaded: bool,
        p: &Parsed,
    ) -> Inserted {
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT id, thread FROM messages WHERE account = ?1 AND message_id = ?2",
                params![account, p.message_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .expect("lookup message");
        if let Some((id, thread)) = existing {
            self.conn
                .execute(
                    "UPDATE messages SET folder = ?2,
                        uid = CASE WHEN folder = ?2 THEN COALESCE(?3, uid) ELSE ?3 END, seen = ?4
                     WHERE id = ?1",
                    params![id, folder, uid, seen],
                )
                .expect("update message");
            if body_loaded {
                self.set_body(id, p);
            }
            return Inserted {
                id,
                thread,
                new: false,
            };
        }
        let thread = self.thread_for(account, p);
        self.conn
            .execute(
                "INSERT INTO messages(account, folder, uid, message_id, thread, parents, subject,
                    nsubject, from_name, from_addr, reply_to, to_json, cc_json, date, preview,
                    text, html, attachments, body, bulk, seen)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                    ?17, ?18, ?19, ?20, ?21)",
                params![
                    account,
                    folder,
                    uid,
                    p.message_id,
                    thread,
                    json(&p.parents),
                    p.subject,
                    normalize_subject(&p.subject),
                    p.from.name,
                    p.from.addr,
                    p.reply_to.as_ref().map(json),
                    json(&p.to),
                    json(&p.cc),
                    p.date,
                    p.preview,
                    body_loaded.then_some(p.text.as_deref()).flatten(),
                    body_loaded.then_some(p.html.as_deref()).flatten(),
                    json(&p.attachments),
                    body_loaded,
                    p.bulk,
                    seen,
                ],
            )
            .expect("insert message");
        let id = self.conn.last_insert_rowid();
        self.index(id, p, body_loaded);
        // A reply in the meantime ends a snooze (TRI-5).
        if folder == INBOX {
            self.conn
                .execute(
                    "UPDATE threads SET snooze_until = NULL WHERE account = ?1 AND thread = ?2",
                    params![account, thread],
                )
                .expect("end snooze");
        }
        Inserted {
            id,
            thread,
            new: true,
        }
    }

    fn index(&self, id: i64, p: &Parsed, with_body: bool) {
        let people: Vec<String> = std::iter::once(&p.from)
            .chain(p.to.iter())
            .chain(p.cc.iter())
            .map(|a| format!("{} {}", a.name, a.addr))
            .collect();
        let body = if with_body {
            p.text
                .clone()
                .or_else(|| p.html.as_deref().map(crate::mail_parse::html_to_text))
                .unwrap_or_default()
        } else {
            p.preview.clone()
        };
        self.conn
            .execute("DELETE FROM fts WHERE rowid = ?1", params![id])
            .expect("fts delete");
        self.conn
            .execute(
                "INSERT INTO fts(rowid, subject, people, body) VALUES(?1, ?2, ?3, ?4)",
                params![id, p.subject, people.join(" "), body],
            )
            .expect("fts insert");
    }

    /// Fill in the body of a message stored with headers only.
    pub fn set_body(&self, id: i64, p: &Parsed) {
        self.conn
            .execute(
                "UPDATE messages SET text = ?2, html = ?3, attachments = ?4, preview = ?5, body = 1
                 WHERE id = ?1",
                params![id, p.text, p.html, json(&p.attachments), p.preview],
            )
            .expect("set body");
        self.index(id, p, true);
    }

    /// Thread of a new message: a stored parent's, else a stored child's
    /// (replies can sync before what they answer), else a recent reply with
    /// the same subject and a shared participant, else its own id.
    fn thread_for(&self, account: &str, p: &Parsed) -> String {
        for parent in &p.parents {
            if let Some(thread) = self.thread_of(account, parent) {
                return thread;
            }
        }
        let child: Option<String> = self
            .conn
            .query_row(
                "SELECT thread FROM messages WHERE account = ?1 AND parents LIKE ?2 LIMIT 1",
                params![account, format!("%{}%", json(&p.message_id))],
                |r| r.get(0),
            )
            .optional()
            .expect("child thread");
        if let Some(thread) = child {
            return thread;
        }
        let nsubject = normalize_subject(&p.subject);
        let is_reply = !p.parents.is_empty() || nsubject != p.subject.trim().to_lowercase();
        if is_reply && !nsubject.is_empty() {
            let people: HashSet<&str> = std::iter::once(&p.from)
                .chain(&p.to)
                .chain(&p.cc)
                .map(|a| a.addr.as_str())
                .collect();
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT thread, from_addr, to_json, cc_json FROM messages
                     WHERE account = ?1 AND nsubject = ?2 AND date BETWEEN ?3 AND ?4
                     ORDER BY date DESC",
                )
                .expect("subject thread");
            let rows = stmt
                .query_map(
                    params![
                        account,
                        nsubject,
                        p.date - SUBJECT_THREAD_WINDOW_SECS,
                        p.date + SUBJECT_THREAD_WINDOW_SECS
                    ],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    },
                )
                .expect("subject thread rows");
            for row in rows {
                let (thread, from, to, cc) = row.expect("subject row");
                let mut theirs: Vec<Addr> = serde_json::from_str(&to).unwrap_or_default();
                theirs.extend(serde_json::from_str::<Vec<Addr>>(&cc).unwrap_or_default());
                if people.contains(from.as_str())
                    || theirs.iter().any(|a| people.contains(a.addr.as_str()))
                {
                    return thread;
                }
            }
        }
        p.message_id.clone()
    }

    pub fn thread_of(&self, account: &str, message_id: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT thread FROM messages WHERE account = ?1 AND message_id = ?2",
                params![account, message_id],
                |r| r.get(0),
            )
            .optional()
            .expect("thread of")
    }

    /// Messages of `folder` whose UIDs the server no longer lists there
    /// moved elsewhere — archived from another client, deleted.
    pub fn left_folder(&self, account: &str, folder: &str, gone: &[u32]) {
        let mut stmt = self
            .conn
            .prepare_cached(
                "UPDATE messages SET folder = 'archive', uid = NULL
                 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            )
            .expect("left folder");
        for uid in gone {
            stmt.execute(params![account, folder, uid])
                .expect("left folder row");
        }
    }

    pub fn set_seen_uids(&self, account: &str, folder: &str, seen: &[(u32, bool)]) {
        let mut stmt = self
            .conn
            .prepare_cached(
                "UPDATE messages SET seen = ?4 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            )
            .expect("seen");
        for (uid, s) in seen {
            stmt.execute(params![account, folder, uid, s])
                .expect("seen row");
        }
    }

    // ── Views ───────────────────────────────────────────────────────────────

    /// The inbox of `accounts` at `now` (spec INB, GK).
    pub fn inbox(&self, accounts: &[AccountRef], now: i64) -> InboxView {
        let mut view = InboxView::default();
        let mut other: HashMap<String, SenderGroup> = HashMap::new();
        for account in accounts {
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT thread FROM messages WHERE account = ?1 AND folder = 'inbox'
                     UNION SELECT thread FROM threads WHERE account = ?1 AND returned = 'reminder'",
                )
                .expect("inbox threads");
            let threads: Vec<String> = stmt
                .query_map(params![account.key], |r| r.get(0))
                .expect("inbox threads")
                .map(|r| r.expect("thread"))
                .collect();
            for thread in threads {
                let Some(row) = self.thread_row(account, &thread, true) else {
                    continue;
                };
                if row.muted {
                    continue;
                }
                let state = self.thread_state(&account.key, &thread);
                if let Some(until) = state.snooze_until.filter(|u| *u > now) {
                    view.next_snooze = Some(view.next_snooze.map_or(until, |n| n.min(until)));
                    continue;
                }
                match self.gate(&account.key, &row.from_addr).as_deref() {
                    Some("blocked") => continue,
                    None if account.gatekeeper && row.from_addr != account.address => {
                        view.new_senders.push(row);
                        continue;
                    }
                    _ => {}
                }
                if row.pinned || row.class == "important" {
                    if row.unread {
                        view.unread_important += 1;
                    }
                    view.important.push(row);
                } else {
                    let group = other
                        .entry(row.from_addr.clone())
                        .or_insert_with(|| SenderGroup {
                            name: row.from_name.clone(),
                            addr: row.from_addr.clone(),
                            unread: 0,
                            threads: Vec::new(),
                        });
                    group.unread += usize::from(row.unread);
                    group.threads.push(row);
                }
            }
        }
        view.important
            .sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.key.cmp(&a.key)));
        view.new_senders.sort_by_key(|t| std::cmp::Reverse(t.key));
        let mut groups: Vec<SenderGroup> = other.into_values().collect();
        for g in &mut groups {
            g.threads.sort_by_key(|t| std::cmp::Reverse(t.key));
        }
        groups.sort_by(|a, b| b.threads[0].key.cmp(&a.threads[0].key));
        view.other = groups;
        view
    }

    /// One thread as an inbox row. `inbox_only` counts unread among inbox
    /// messages only.
    pub fn thread_row(
        &self,
        account: &AccountRef,
        thread: &str,
        inbox_only: bool,
    ) -> Option<ThreadRow> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT subject, from_name, from_addr, date, preview, attachments, bulk, seen, folder
                 FROM messages WHERE account = ?1 AND thread = ?2 ORDER BY date ASC",
            )
            .expect("thread row");
        let rows: Vec<RowMessage> = stmt
            .query_map(params![account.key, thread], |r| {
                Ok(RowMessage {
                    subject: r.get(0)?,
                    from_name: r.get(1)?,
                    from_addr: r.get(2)?,
                    date: r.get(3)?,
                    preview: r.get(4)?,
                    attachments: r.get(5)?,
                    bulk: r.get(6)?,
                    seen: r.get(7)?,
                    folder: r.get(8)?,
                })
            })
            .expect("thread rows")
            .map(|r| r.expect("thread message"))
            .collect();
        let latest = rows.last()?;
        let theirs = rows
            .iter()
            .rev()
            .find(|m| m.from_addr != account.address)
            .unwrap_or(latest);
        let state = self.thread_state(&account.key, thread);
        let class = match self
            .sender_class(&account.key, &theirs.from_addr)
            .as_deref()
        {
            Some("important") => "important",
            Some("other") => "other",
            _ if theirs.bulk => "other",
            _ => "important",
        };
        let unread = rows
            .iter()
            .any(|m| !m.seen && (!inbox_only || m.folder == INBOX));
        let date = latest.date;
        Some(ThreadRow {
            account: account.key.clone(),
            thread: thread.to_string(),
            subject: rows[0].subject.clone(),
            from_name: if theirs.from_name.is_empty() {
                theirs.from_addr.clone()
            } else {
                theirs.from_name.clone()
            },
            from_addr: theirs.from_addr.clone(),
            count: rows.len(),
            unread,
            date,
            preview: latest.preview.clone(),
            attachment: rows.iter().any(|m| m.attachments != "[]"),
            pinned: state.pinned,
            muted: state.muted,
            class,
            key: state.returned_at.map_or(date, |r| r.max(date)),
            returned: state.returned,
            remind_at: state.remind_at,
            summary: state.summary,
        })
    }

    pub fn thread(&self, account: &str, thread: &str) -> Vec<MessageView> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, message_id, folder, uid, from_name, from_addr, reply_to, to_json,
                    cc_json, subject, date, text, html, attachments, body, seen, parents
                 FROM messages WHERE account = ?1 AND thread = ?2 ORDER BY date ASC, id ASC",
            )
            .expect("thread");
        stmt.query_map(params![account, thread], |r| {
            Ok(MessageView {
                id: r.get(0)?,
                message_id: r.get(1)?,
                folder: r.get(2)?,
                uid: r.get(3)?,
                from: Addr {
                    name: r.get(4)?,
                    addr: r.get(5)?,
                },
                reply_to: r
                    .get::<_, Option<String>>(6)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                to: from_json(&r.get::<_, String>(7)?),
                cc: from_json(&r.get::<_, String>(8)?),
                subject: r.get(9)?,
                date: r.get(10)?,
                text: r.get(11)?,
                html: r.get(12)?,
                attachments: from_json(&r.get::<_, String>(13)?),
                body_loaded: r.get(14)?,
                seen: r.get(15)?,
                parents: from_json(&r.get::<_, String>(16)?),
            })
        })
        .expect("thread messages")
        .map(|r| r.expect("message"))
        .collect()
    }

    /// Full-text and field search over every stored message of `accounts`,
    /// one row per thread, newest first (spec SRCH-1, SRCH-2).
    pub fn search(&self, accounts: &[AccountRef], query: &str, limit: usize) -> Vec<ThreadRow> {
        let q = SearchQuery::parse(query);
        let mut sql = String::from(
            "SELECT m.account, m.thread, MAX(m.date) AS d FROM messages m WHERE m.account IN (",
        );
        sql.push_str(&vec!["?"; accounts.len()].join(","));
        sql.push(')');
        let mut args: Vec<rusqlite::types::Value> =
            accounts.iter().map(|a| a.key.clone().into()).collect();
        if !q.words.is_empty() {
            sql.push_str(" AND m.id IN (SELECT rowid FROM fts WHERE fts MATCH ?)");
            args.push(q.fts().into());
        }
        if let Some(from) = &q.from {
            sql.push_str(" AND (m.from_addr LIKE ? OR m.from_name LIKE ?)");
            args.push(format!("%{from}%").into());
            args.push(format!("%{from}%").into());
        }
        if let Some(to) = &q.to {
            sql.push_str(" AND (m.to_json LIKE ? OR m.cc_json LIKE ?)");
            args.push(format!("%{to}%").into());
            args.push(format!("%{to}%").into());
        }
        if q.attachment {
            sql.push_str(" AND m.attachments != '[]'");
        }
        if q.unread {
            sql.push_str(" AND m.seen = 0");
        }
        if let Some(before) = q.before {
            sql.push_str(" AND m.date < ?");
            args.push(before.into());
        }
        if let Some(after) = q.after {
            sql.push_str(" AND m.date >= ?");
            args.push(after.into());
        }
        sql.push_str(" GROUP BY m.account, m.thread ORDER BY d DESC LIMIT ?");
        args.push((limit as i64).into());
        let Ok(mut stmt) = self.conn.prepare(&sql) else {
            return Vec::new();
        };
        let hits: Vec<(String, String)> = match stmt
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok((r.get(0)?, r.get(1)?))
            }) {
            Ok(rows) => rows.filter_map(Result::ok).collect(),
            // A malformed FTS query is the user's typo, not an error.
            Err(_) => return Vec::new(),
        };
        hits.into_iter()
            .filter_map(|(account, thread)| {
                let account = accounts.iter().find(|a| a.key == account)?;
                self.thread_row(account, &thread, false)
            })
            .collect()
    }

    /// Bodies of the latest messages the account sent to `addr`, newest
    /// first: the user's own voice, for AI drafts (AI-3).
    pub fn sent_to(&self, account: &str, addr: &str, limit: usize) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT text FROM messages
                 WHERE account = ?1 AND folder = 'sent' AND text IS NOT NULL AND to_json LIKE ?2
                 ORDER BY date DESC LIMIT ?3",
            )
            .expect("sent to");
        stmt.query_map(
            params![account, format!("%\"{addr}\"%"), limit as i64],
            |r| r.get(0),
        )
        .expect("sent to")
        .map(|r| r.expect("sent text"))
        .collect()
    }

    /// Messages of a thread still in the inbox, for Done and Mute.
    pub fn inbox_refs(&self, account: &str, thread: &str) -> Vec<ServerRef> {
        self.refs_where(account, thread, "folder = 'inbox'")
    }

    pub fn refs(&self, account: &str, thread: &str) -> Vec<ServerRef> {
        self.refs_where(account, thread, "1")
    }

    fn refs_where(&self, account: &str, thread: &str, filter: &str) -> Vec<ServerRef> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT uid, folder, message_id FROM messages
                 WHERE account = ?1 AND thread = ?2 AND {filter}"
            ))
            .expect("refs");
        stmt.query_map(params![account, thread], |r| {
            Ok(ServerRef {
                uid: r.get(0)?,
                folder: r.get(1)?,
                message_id: r.get(2)?,
            })
        })
        .expect("refs")
        .map(|r| r.expect("ref"))
        .collect()
    }

    /// Where the store believes messages are now, ahead of the server move.
    pub fn move_local(&self, account: &str, message_ids: &[String], folder: &str) {
        let mut stmt = self
            .conn
            .prepare_cached(
                "UPDATE messages SET folder = ?3, uid = CASE WHEN ?3 = folder THEN uid ELSE NULL END
                 WHERE account = ?1 AND message_id = ?2",
            )
            .expect("move local");
        for id in message_ids {
            stmt.execute(params![account, id, folder])
                .expect("move local row");
        }
    }

    pub fn set_seen(&self, account: &str, message_ids: &[String], seen: bool) {
        let mut stmt = self
            .conn
            .prepare_cached("UPDATE messages SET seen = ?3 WHERE account = ?1 AND message_id = ?2")
            .expect("set seen");
        for id in message_ids {
            stmt.execute(params![account, id, seen])
                .expect("set seen row");
        }
    }

    pub fn message_by_id(&self, id: i64) -> Option<(String, String, Option<u32>, String)> {
        self.conn
            .query_row(
                "SELECT account, folder, uid, message_id FROM messages WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .expect("message by id")
    }

    // ── Thread state ────────────────────────────────────────────────────────

    pub fn thread_state(&self, account: &str, thread: &str) -> ThreadState {
        self.conn
            .query_row(
                "SELECT pinned, muted, snooze_until, returned, returned_at, remind_at, summary
                 FROM threads WHERE account = ?1 AND thread = ?2",
                params![account, thread],
                |r| {
                    Ok(ThreadState {
                        pinned: r.get(0)?,
                        muted: r.get(1)?,
                        snooze_until: r.get(2)?,
                        returned: r.get(3)?,
                        returned_at: r.get(4)?,
                        remind_at: r.get(5)?,
                        summary: r.get(6)?,
                    })
                },
            )
            .optional()
            .expect("thread state")
            .unwrap_or_default()
    }

    fn update_thread(&self, account: &str, thread: &str, set: &str, value: rusqlite::types::Value) {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO threads(account, thread) VALUES(?1, ?2)",
                params![account, thread],
            )
            .expect("thread row");
        self.conn
            .execute(
                &format!("UPDATE threads SET {set} = ?3 WHERE account = ?1 AND thread = ?2"),
                params![account, thread, value],
            )
            .expect("update thread");
    }

    pub fn set_pinned(&self, account: &str, thread: &str, on: bool) {
        self.update_thread(account, thread, "pinned", i64::from(on).into());
    }

    pub fn set_muted(&self, account: &str, thread: &str, on: bool) {
        self.update_thread(account, thread, "muted", i64::from(on).into());
    }

    pub fn set_summary(&self, account: &str, thread: &str, summary: &str) {
        self.update_thread(account, thread, "summary", summary.to_string().into());
    }

    /// Snooze until `until`, or wake now with `None`. Either way the thread
    /// is no longer marked as having come back.
    pub fn set_snooze(&self, account: &str, thread: &str, until: Option<i64>) {
        self.update_thread(account, thread, "snooze_until", until.into());
        self.update_thread(account, thread, "returned", rusqlite::types::Value::Null);
    }

    /// Snoozes due at `now` come back marked as returned (TRI-5).
    pub fn wake_snoozes(&self, now: i64) -> Vec<(String, String)> {
        let due = self.due("snooze_until", now);
        for (account, thread) in &due {
            self.conn
                .execute(
                    "UPDATE threads SET returned = 'snooze', returned_at = snooze_until,
                        snooze_until = NULL WHERE account = ?1 AND thread = ?2",
                    params![account, thread],
                )
                .expect("wake snooze");
        }
        due
    }

    /// Return `thread` to the inbox at `at` unless someone else writes after
    /// `since` (WRT-6).
    pub fn set_reminder(&self, account: &str, thread: &str, at: i64, since: i64) {
        self.update_thread(account, thread, "remind_at", at.into());
        self.update_thread(account, thread, "remind_since", since.into());
    }

    pub fn clear_reminder(&self, account: &str, thread: &str) {
        self.update_thread(account, thread, "remind_at", rusqlite::types::Value::Null);
        self.update_thread(account, thread, "returned", rusqlite::types::Value::Null);
    }

    /// Reminders due at `now`: unanswered threads come back to the inbox;
    /// answered ones are dropped quietly.
    pub fn fire_reminders(&self, now: i64, own: &HashMap<String, String>) -> Vec<(String, String)> {
        let mut fired = Vec::new();
        for (account, thread) in self.due("remind_at", now) {
            let since: i64 = self
                .conn
                .query_row(
                    "SELECT COALESCE(remind_since, 0) FROM threads WHERE account = ?1 AND thread = ?2",
                    params![account, thread],
                    |r| r.get(0),
                )
                .expect("remind since");
            let me = own.get(&account).map(String::as_str).unwrap_or_default();
            let answered: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM messages WHERE account = ?1 AND thread = ?2
                        AND date > ?3 AND from_addr != ?4)",
                    params![account, thread, since, me],
                    |r| r.get(0),
                )
                .expect("answered");
            self.conn
                .execute(
                    "UPDATE threads SET remind_at = NULL,
                        returned = CASE WHEN ?3 THEN returned ELSE 'reminder' END,
                        returned_at = CASE WHEN ?3 THEN returned_at ELSE ?4 END
                     WHERE account = ?1 AND thread = ?2",
                    params![account, thread, answered, now],
                )
                .expect("fire reminder");
            if !answered {
                fired.push((account, thread));
            }
        }
        fired
    }

    fn due(&self, column: &str, now: i64) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT account, thread FROM threads WHERE {column} IS NOT NULL AND {column} <= ?1"
            ))
            .expect("due");
        stmt.query_map(params![now], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("due")
            .map(|r| r.expect("due row"))
            .collect()
    }

    /// Next time a snooze or reminder falls due, to sleep until then.
    pub fn next_due(&self) -> Option<i64> {
        self.conn
            .query_row(
                "SELECT MIN(t) FROM (SELECT snooze_until AS t FROM threads WHERE snooze_until IS NOT NULL
                   UNION ALL SELECT remind_at FROM threads WHERE remind_at IS NOT NULL
                   UNION ALL SELECT send_at FROM outbox)",
                [],
                |r| r.get(0),
            )
            .expect("next due")
    }

    /// Which messages Done moved, so undo can bring the same ones back.
    pub fn set_done(&self, account: &str, thread: &str, ids: &[String]) {
        self.update_thread(account, thread, "done", json(&ids).into());
        self.update_thread(account, thread, "returned", rusqlite::types::Value::Null);
    }

    pub fn done_ids(&self, account: &str, thread: &str) -> Vec<String> {
        self.conn
            .query_row(
                "SELECT done FROM threads WHERE account = ?1 AND thread = ?2",
                params![account, thread],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .expect("done ids")
            .flatten()
            .map(|s| from_json(&s))
            .unwrap_or_default()
    }

    // ── Senders ─────────────────────────────────────────────────────────────

    fn sender(&self, account: &str, addr: &str, column: &str) -> Option<String> {
        self.conn
            .query_row(
                &format!("SELECT {column} FROM senders WHERE account = ?1 AND addr = ?2"),
                params![account, addr],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .expect("sender")
            .flatten()
    }

    fn set_sender(&self, account: &str, addr: &str, column: &str, value: Option<&str>) {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO senders(account, addr) VALUES(?1, ?2)",
                params![account, addr],
            )
            .expect("sender row");
        self.conn
            .execute(
                &format!("UPDATE senders SET {column} = ?3 WHERE account = ?1 AND addr = ?2"),
                params![account, addr, value],
            )
            .expect("set sender");
    }

    /// `important` or `other` when the user moved the sender (INB-4).
    pub fn sender_class(&self, account: &str, addr: &str) -> Option<String> {
        self.sender(account, addr, "class")
    }

    pub fn set_sender_class(&self, account: &str, addr: &str, class: Option<&str>) {
        self.set_sender(account, addr, "class", class);
    }

    /// `accepted` or `blocked` (GK-2, TRI-7).
    pub fn gate(&self, account: &str, addr: &str) -> Option<String> {
        self.sender(account, addr, "gate")
    }

    pub fn set_gate(&self, account: &str, addr: &str, gate: Option<&str>) {
        self.set_sender(account, addr, "gate", gate);
    }

    pub fn images_allowed(&self, account: &str, addr: &str) -> bool {
        self.sender(account, addr, "images")
            .is_some_and(|v| v == "1")
    }

    pub fn allow_images(&self, account: &str, addr: &str) {
        self.set_sender(account, addr, "images", Some("1"));
    }

    /// Everyone the account has heard from or written to so far is known:
    /// Gatekeeper only screens senders new after it is turned on (GK-1).
    pub fn accept_known_senders(&self, account: &str) {
        let mut addrs: HashSet<String> = HashSet::new();
        let mut stmt = self
            .conn
            .prepare("SELECT from_addr, to_json, cc_json, folder FROM messages WHERE account = ?1")
            .expect("known senders");
        let rows = stmt
            .query_map(params![account], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .expect("known senders");
        for row in rows {
            let (from, to, cc, folder) = row.expect("known sender");
            addrs.insert(from);
            if folder == SENT {
                addrs.extend(from_json::<Addr>(&to).into_iter().map(|a| a.addr));
                addrs.extend(from_json::<Addr>(&cc).into_iter().map(|a| a.addr));
            }
        }
        for addr in addrs {
            if self.gate(account, &addr).is_none() {
                self.set_gate(account, &addr, Some("accepted"));
            }
        }
    }

    // ── Outbox, drafts, queued actions, meta ────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn queue_send(
        &self,
        account: &str,
        raw: &[u8],
        send_at: i64,
        remind_after: Option<i64>,
        thread: Option<&str>,
        message_id: &str,
        recipients: &[String],
    ) -> i64 {
        self.conn
            .execute(
                "INSERT INTO outbox(account, raw, send_at, remind_after, thread, message_id, recipients)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![account, raw, send_at, remind_after, thread, message_id, json(&recipients)],
            )
            .expect("queue send");
        self.conn.last_insert_rowid()
    }

    /// Take a message back before it leaves; false once it is gone.
    pub fn cancel_send(&self, id: i64) -> bool {
        self.conn
            .execute("DELETE FROM outbox WHERE id = ?1", params![id])
            .expect("cancel send")
            > 0
    }

    pub fn due_sends(&self, now: i64) -> Vec<Outgoing> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, account, raw, send_at, remind_after, thread, message_id, recipients
                 FROM outbox WHERE send_at <= ?1 ORDER BY send_at",
            )
            .expect("due sends");
        stmt.query_map(params![now], |r| {
            Ok(Outgoing {
                id: r.get(0)?,
                account: r.get(1)?,
                raw: r.get(2)?,
                send_at: r.get(3)?,
                remind_after: r.get(4)?,
                thread: r.get(5)?,
                message_id: r.get(6)?,
                recipients: from_json(&r.get::<_, String>(7)?),
            })
        })
        .expect("due sends")
        .map(|r| r.expect("outgoing"))
        .collect()
    }

    pub fn scheduled(&self) -> Vec<(i64, String, i64, Option<String>)> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, account, send_at, thread FROM outbox ORDER BY send_at")
            .expect("scheduled");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .expect("scheduled")
            .map(|r| r.expect("scheduled row"))
            .collect()
    }

    pub fn sent(&self, id: i64) {
        self.cancel_send(id);
    }

    pub fn send_failed(&self, id: i64, error: &str) {
        self.conn
            .execute(
                "UPDATE outbox SET error = ?2 WHERE id = ?1",
                params![id, error],
            )
            .expect("send failed");
    }

    pub fn save_draft(&self, key: &str, json_text: &str, now: i64) {
        self.conn
            .execute(
                "INSERT INTO drafts(key, json, updated) VALUES(?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET json = excluded.json, updated = excluded.updated",
                params![key, json_text, now],
            )
            .expect("save draft");
    }

    pub fn delete_draft(&self, key: &str) {
        self.conn
            .execute("DELETE FROM drafts WHERE key = ?1", params![key])
            .expect("delete draft");
    }

    pub fn draft(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT json FROM drafts WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
            .expect("draft")
    }

    pub fn drafts(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, json FROM drafts ORDER BY updated DESC")
            .expect("drafts");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("drafts")
            .map(|r| r.expect("draft row"))
            .collect()
    }

    pub fn queue_action(&self, account: &str, kind: &str, refs: &[ServerRef]) {
        if refs.is_empty() {
            return;
        }
        self.conn
            .execute(
                "INSERT INTO actions(account, kind, refs) VALUES(?1, ?2, ?3)",
                params![account, kind, json(&refs)],
            )
            .expect("queue action");
    }

    pub fn actions(&self, account: &str) -> Vec<QueuedAction> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, kind, refs FROM actions WHERE account = ?1 ORDER BY id")
            .expect("actions");
        stmt.query_map(params![account], |r| {
            Ok(QueuedAction {
                id: r.get(0)?,
                kind: r.get(1)?,
                refs: from_json(&r.get::<_, String>(2)?),
            })
        })
        .expect("actions")
        .map(|r| r.expect("action"))
        .collect()
    }

    pub fn action_done(&self, id: i64) {
        self.conn
            .execute("DELETE FROM actions WHERE id = ?1", params![id])
            .expect("action done");
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()
            .expect("meta")
    }

    pub fn set_meta(&self, key: &str, value: &str) {
        self.conn
            .execute(
                "INSERT INTO meta(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .expect("set meta");
    }

    /// Everything stored for an account (SYN-6).
    pub fn forget_account(&self, account: &str) {
        self.conn
            .execute(
                "DELETE FROM fts WHERE rowid IN (SELECT id FROM messages WHERE account = ?1)",
                params![account],
            )
            .expect("forget fts");
        for table in [
            "messages", "threads", "senders", "folders", "outbox", "actions",
        ] {
            self.conn
                .execute(
                    &format!("DELETE FROM {table} WHERE account = ?1"),
                    params![account],
                )
                .expect("forget account");
        }
        self.conn
            .execute(
                "DELETE FROM meta WHERE key LIKE ?1",
                params![format!("%:{account}")],
            )
            .expect("forget meta");
    }
}

fn json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("serialize")
}

fn from_json<T: serde::de::DeserializeOwned>(s: &str) -> Vec<T> {
    serde_json::from_str(s).unwrap_or_default()
}

/// Spec SRCH-2: `from:`, `to:`, `has:attachment`, `is:unread`, `before:`,
/// `after:` (YYYY-MM-DD) and free words.
#[derive(Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub words: Vec<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub attachment: bool,
    pub unread: bool,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

impl SearchQuery {
    pub fn parse(query: &str) -> Self {
        let mut q = Self::default();
        for token in query.split_whitespace() {
            let lower = token.to_lowercase();
            if let Some(v) = lower.strip_prefix("from:") {
                q.from = Some(v.to_string());
            } else if let Some(v) = lower.strip_prefix("to:") {
                q.to = Some(v.to_string());
            } else if lower == "has:attachment" {
                q.attachment = true;
            } else if lower == "is:unread" {
                q.unread = true;
            } else if let Some(v) = lower.strip_prefix("before:") {
                q.before = day_start(v);
            } else if let Some(v) = lower.strip_prefix("after:") {
                q.after = day_start(v);
            } else {
                q.words.push(token.to_string());
            }
        }
        q
    }

    /// Words as FTS5 prefix terms, quoted so punctuation is never syntax.
    fn fts(&self) -> String {
        self.words
            .iter()
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Unix seconds at UTC midnight of `YYYY-MM-DD`.
fn day_start(date: &str) -> Option<i64> {
    let mut parts = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, d) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil, Howard Hinnant's algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, parents: &[&str], from: &str, subject: &str, date: i64, bulk: bool) -> Parsed {
        Parsed {
            message_id: id.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            from: Addr {
                name: from.split('@').next().unwrap().into(),
                addr: from.into(),
            },
            to: vec![Addr {
                name: String::new(),
                addr: "me@x.org".into(),
            }],
            subject: subject.into(),
            date,
            preview: format!("preview of {id}"),
            text: Some(format!("body of {id}")),
            bulk,
            ..Default::default()
        }
    }

    fn acct(gatekeeper: bool) -> AccountRef {
        AccountRef {
            key: "a".into(),
            address: "me@x.org".into(),
            gatekeeper,
        }
    }

    #[test]
    fn replies_thread_by_reference_in_any_order() {
        let s = Store::in_memory();
        let child = s.insert(
            "a",
            INBOX,
            Some(2),
            false,
            true,
            &msg("c", &["p"], "bob@y", "Re: Plan", 20, false),
        );
        let parent = s.insert(
            "a",
            INBOX,
            Some(1),
            true,
            true,
            &msg("p", &[], "anna@y", "Plan", 10, false),
        );
        assert_eq!(child.thread, "c");
        assert_eq!(
            parent.thread, "c",
            "a parent synced late joins its reply's thread"
        );
        let later = s.insert(
            "a",
            INBOX,
            Some(3),
            false,
            true,
            &msg("d", &["c"], "anna@y", "Re: Plan", 30, false),
        );
        assert_eq!(later.thread, "c");
        assert_eq!(s.thread("a", "c").len(), 3);
    }

    #[test]
    fn subject_fallback_needs_a_reply_and_a_shared_person() {
        let s = Store::in_memory();
        s.insert(
            "a",
            INBOX,
            Some(1),
            true,
            true,
            &msg("p", &[], "anna@y", "Invoice", 10, false),
        );
        let reply = s.insert(
            "a",
            INBOX,
            Some(2),
            true,
            true,
            &msg("r", &[], "anna@y", "Re: Invoice", 20, false),
        );
        assert_eq!(reply.thread, "p");
        let stranger = s.insert(
            "a",
            INBOX,
            Some(3),
            true,
            true,
            &msg("q", &[], "eve@z", "Invoice", 30, false),
        );
        assert_eq!(stranger.thread, "q");
    }

    #[test]
    fn inbox_splits_important_other_pins_and_snoozes() {
        let s = Store::in_memory();
        s.insert(
            "a",
            INBOX,
            Some(1),
            false,
            true,
            &msg("m1", &[], "anna@y", "Hi", 10, false),
        );
        s.insert(
            "a",
            INBOX,
            Some(2),
            false,
            true,
            &msg("m2", &[], "news@shop", "Sale", 20, true),
        );
        s.insert(
            "a",
            INBOX,
            Some(3),
            true,
            true,
            &msg("m3", &[], "bob@y", "Old", 5, false),
        );
        s.insert(
            "a",
            INBOX,
            Some(4),
            true,
            true,
            &msg("m4", &[], "carl@y", "Later", 15, false),
        );
        s.set_pinned("a", "m3", true);
        s.set_snooze("a", "m4", Some(1000));

        let v = s.inbox(&[acct(false)], 100);
        let important: Vec<&str> = v.important.iter().map(|t| t.thread.as_str()).collect();
        assert_eq!(important, vec!["m3", "m1"], "pinned first, then newest");
        assert_eq!(v.other.len(), 1);
        assert_eq!(v.other[0].addr, "news@shop");
        assert_eq!(v.next_snooze, Some(1000));
        assert_eq!(v.unread_important, 1);

        s.set_sender_class("a", "news@shop", Some("important"));
        assert!(
            s.inbox(&[acct(false)], 100).other.is_empty(),
            "INB-4 moves the sender"
        );

        assert_eq!(
            s.wake_snoozes(1000),
            vec![("a".to_string(), "m4".to_string())]
        );
        let woke = s.inbox(&[acct(false)], 1001);
        let row = woke.important.iter().find(|t| t.thread == "m4").unwrap();
        assert_eq!(row.returned.as_deref(), Some("snooze"));
        assert_eq!(row.key, 1000);
    }

    #[test]
    fn done_and_moves_leave_the_inbox() {
        let s = Store::in_memory();
        s.insert(
            "a",
            INBOX,
            Some(1),
            false,
            true,
            &msg("m1", &[], "anna@y", "Hi", 10, false),
        );
        let refs = s.inbox_refs("a", "m1");
        assert_eq!(
            refs,
            vec![ServerRef {
                uid: Some(1),
                folder: INBOX.into(),
                message_id: "m1".into()
            }]
        );
        s.move_local("a", &["m1".to_string()], ARCHIVE);
        assert!(s.inbox(&[acct(false)], 100).important.is_empty());
        s.move_local("a", &["m1".to_string()], INBOX);
        assert_eq!(s.inbox(&[acct(false)], 100).important.len(), 1);
    }

    #[test]
    fn gatekeeper_holds_only_senders_new_since_it_was_turned_on() {
        let s = Store::in_memory();
        s.insert(
            "a",
            INBOX,
            Some(1),
            false,
            true,
            &msg("m1", &[], "anna@y", "Hi", 10, false),
        );
        s.accept_known_senders("a");
        s.insert(
            "a",
            INBOX,
            Some(2),
            false,
            true,
            &msg("m2", &[], "new@z", "Hello", 20, false),
        );
        let v = s.inbox(&[acct(true)], 100);
        assert_eq!(v.new_senders.len(), 1);
        assert_eq!(v.important.len(), 1);
        s.set_gate("a", "new@z", Some("blocked"));
        let v = s.inbox(&[acct(true)], 100);
        assert!(v.new_senders.is_empty());
        assert_eq!(v.important.len(), 1);
    }

    #[test]
    fn reminders_return_only_unanswered_threads() {
        let s = Store::in_memory();
        s.insert(
            "a",
            SENT,
            Some(1),
            true,
            true,
            &msg("s1", &[], "me@x.org", "Question", 10, false),
        );
        s.insert(
            "a",
            SENT,
            Some(2),
            true,
            true,
            &msg("s2", &[], "me@x.org", "Other", 10, false),
        );
        s.set_reminder("a", "s1", 100, 10);
        s.set_reminder("a", "s2", 100, 10);
        s.insert(
            "a",
            INBOX,
            Some(3),
            false,
            true,
            &msg("r2", &["s2"], "bob@y", "Re: Other", 50, false),
        );
        let own = HashMap::from([("a".to_string(), "me@x.org".to_string())]);
        assert_eq!(
            s.fire_reminders(100, &own),
            vec![("a".to_string(), "s1".to_string())]
        );
        let v = s.inbox(&[acct(false)], 101);
        let row = v.important.iter().find(|t| t.thread == "s1").unwrap();
        assert_eq!(row.returned.as_deref(), Some("reminder"));
    }

    #[test]
    fn search_understands_fields_and_words() {
        let s = Store::in_memory();
        s.insert(
            "a",
            INBOX,
            Some(1),
            false,
            true,
            &msg("m1", &[], "anna@y", "Budget review", 10, false),
        );
        s.insert(
            "a",
            ARCHIVE,
            Some(2),
            true,
            true,
            &msg("m2", &[], "bob@y", "Holiday", 20, false),
        );
        let hits = |q: &str| -> Vec<String> {
            s.search(&[acct(false)], q, 10)
                .into_iter()
                .map(|t| t.thread)
                .collect()
        };
        assert_eq!(hits("budg"), vec!["m1"]);
        assert_eq!(hits("from:bob"), vec!["m2"]);
        assert_eq!(hits("is:unread"), vec!["m1"]);
        assert_eq!(hits("body"), vec!["m2", "m1"]);
        assert!(hits("\"unbalanced").is_empty());
        assert_eq!(
            SearchQuery::parse("after:2026-10-13").after,
            Some(1_791_849_600)
        );
    }

    #[test]
    fn outbox_and_actions_round_trip() {
        let s = Store::in_memory();
        let id = s.queue_send(
            "a",
            b"raw",
            50,
            Some(3600),
            Some("t"),
            "mid",
            &["x@y".to_string()],
        );
        assert!(s.due_sends(49).is_empty());
        assert_eq!(s.due_sends(50)[0].id, id);
        assert!(s.cancel_send(id));
        assert!(!s.cancel_send(id));
        let r = ServerRef {
            uid: Some(1),
            folder: INBOX.into(),
            message_id: "m".into(),
        };
        s.queue_action("a", "archive", std::slice::from_ref(&r));
        assert_eq!(s.pending_ids("a"), HashSet::from(["m".to_string()]));
        let a = s.actions("a");
        s.action_done(a[0].id);
        assert!(s.actions("a").is_empty());
    }
}
