//! End-to-end test of the mail engine against a real IMAP/SMTP server.
//! Ignored by default; run with a GreenMail container up:
//!
//! ```sh
//! docker run -d --name octoweb-greenmail -p 3025:3025 -p 3143:3143 \
//!   -e GREENMAIL_OPTS='-Dgreenmail.setup.test.smtp -Dgreenmail.setup.test.imap
//!     -Dgreenmail.hostname=0.0.0.0 -Dgreenmail.users=me:secret@local.test,anna:secret@local.test
//!     -Dgreenmail.users.login=email' greenmail/standalone:2.1.3
//! cargo test mail_e2e -- --ignored --nocapture
//! ```

use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures::StreamExt;

use crate::config::{MailAccount, MailProvider, SmtpSecurity};
use crate::mail::{Account, Command, Draft, DraftFile, Event, Item, Mail, PageRequest, Verb};

const IMAP: (&str, u16) = ("127.0.0.1", 3143);
const SMTP_PORT: u16 = 3025;
const WAIT: Duration = Duration::from_secs(30);

fn account(address: &str, gatekeeper: bool) -> Account {
	let mut a = MailAccount {
		id: [7; 16],
		provider: MailProvider::Other,
		address: address.into(),
		name: "Me".into(),
		color: String::new(),
		signature: "Me".into(),
		gatekeeper,
		scope: Default::default(),
		imap_host: IMAP.0.into(),
		imap_port: IMAP.1,
		smtp_host: "127.0.0.1".into(),
		smtp_port: SMTP_PORT,
		smtp_security: SmtpSecurity::Plain,
		login: String::new(),
		archive_folder: String::new(),
		has_password: true,
		password: None,

		auth: Default::default(),
		calendar: true,
		caldav_url: String::new(),

		oauth_client_id: String::new(),

		has_client_secret: false,

		client_secret: None,
	};
	if address != "me@local.test" {
		a.id = [8; 16];
	}
	Account::resolve(&a, "secret".into())
}

fn rt() -> tokio::runtime::Runtime {
	tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.unwrap()
}

/// Deliver a message over SMTP, as any sender.
fn deliver(from: &str, to: &str, subject: &str, body: &str, in_reply_to: Option<&str>, attachment: bool) -> String {
	use lettre::message::{header::ContentType, Attachment, MultiPart, SinglePart};
	use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
	let id = format!("{}@test", crate::mail::hex(&crate::workspace::random_bytes16()));
	let mut b = lettre::Message::builder()
		.from(from.parse().unwrap())
		.to(to.parse().unwrap())
		.subject(subject)
		.message_id(Some(format!("<{id}>")));
	if let Some(parent) = in_reply_to {
		b = b.in_reply_to(format!("<{parent}>")).references(format!("<{parent}>"));
	}
	let msg = if attachment {
		b.multipart(
			MultiPart::mixed()
				.singlepart(SinglePart::plain(body.to_string()))
				.singlepart(Attachment::new("notes.txt".into()).body(b"attached!".to_vec(), ContentType::TEXT_PLAIN)),
		)
	} else {
		b.body(body.to_string())
	}
	.unwrap();
	let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous("127.0.0.1")
		.port(SMTP_PORT)
		.build();
	rt().block_on(transport.send(msg)).unwrap();
	id
}

/// Message-IDs in `folder` of `user`, with In-Reply-To values.
fn server(user: &str, folder: &str) -> Vec<(String, String)> {
	rt().block_on(async {
		let tcp = tokio::net::TcpStream::connect(IMAP).await.unwrap();
		let mut client = async_imap::Client::new(tcp);
		client.read_response().await.unwrap();
		let mut s = client.login(user, "secret").await.map_err(|(e, _)| e).unwrap();
		if s.select(folder).await.is_err() {
			return Vec::new();
		}
		let out: Vec<(String, String)> = {
			let fetched: Vec<_> = s
				.fetch("1:*", "(BODY.PEEK[HEADER.FIELDS (MESSAGE-ID IN-REPLY-TO)])")
				.await
				.unwrap()
				.collect()
				.await;
			fetched
				.into_iter()
				.filter_map(Result::ok)
				.filter_map(|f| {
					let h = String::from_utf8_lossy(f.header().or(f.body())?).to_string();
					let field = |name: &str| {
						h.lines()
							.find(|l| l.to_ascii_lowercase().starts_with(name))
							.map(|l| crate::mail_parse::normalize_id(&l[name.len()..]))
							.unwrap_or_default()
					};
					Some((field("message-id:"), field("in-reply-to:")))
				})
				.collect()
		};
		let _ = s.logout().await;
		out
	})
}

/// File an old message straight into `folder` on the server, as mail from
/// before the sync window is.
fn file_old(user: &str, folder: &str, subject: &str) {
	rt().block_on(async {
		let tcp = tokio::net::TcpStream::connect(IMAP).await.unwrap();
		let mut client = async_imap::Client::new(tcp);
		client.read_response().await.unwrap();
		let mut s = client.login(user, "secret").await.map_err(|(e, _)| e).unwrap();
		let _ = s.create(folder).await;
		let raw = format!(
			"From: Old Friend <old@else.test>\r\nTo: {user}\r\nSubject: {subject}\r\n\
             Message-ID: <old-{subject}@test>\r\nDate: Mon, 6 Jan 2020 10:00:00 +0000\r\n\r\nFrom long ago.\r\n"
		);
		s.append(folder, Some("(\\Seen)"), None, raw.as_bytes()).await.unwrap();
		let _ = s.logout().await;
	});
}

/// Empty `user`'s INBOX and Archive.
fn purge(user: &str) {
	rt().block_on(async {
		let tcp = tokio::net::TcpStream::connect(IMAP).await.unwrap();
		let mut client = async_imap::Client::new(tcp);
		client.read_response().await.unwrap();
		let mut s = client.login(user, "secret").await.map_err(|(e, _)| e).unwrap();
		for folder in ["INBOX", "Archive", "Sent"] {
			if s.select(folder).await.is_ok() {
				let _: Vec<_> = s
					.store("1:*", "+FLAGS.SILENT (\\Deleted)")
					.await
					.unwrap()
					.collect()
					.await;
				let _: Vec<_> = s.expunge().await.unwrap().collect().await;
			}
		}
		let _ = s.create("Sent").await;
		let _ = s.logout().await;
	});
}

struct Harness {
	mail: Mail,
	events: mpsc::Receiver<Event>,
	key: String,
	_dir: std::path::PathBuf,
}

impl Harness {
	fn start(gatekeeper: bool) -> Self {
		let dir = std::env::temp_dir().join(format!(
			"octoweb-mail-e2e-{}",
			crate::mail::hex(&crate::workspace::random_bytes16())
		));
		let (tx, events) = mpsc::channel();
		let tx = std::sync::Mutex::new(tx);
		let mail = Mail::start(dir.join("mail.sqlite"), move |e| {
			let _ = tx.lock().unwrap().send(e);
		})
		.unwrap();
		let acct = account("me@local.test", gatekeeper);
		let key = acct.key.clone();
		mail.send(Command::SetAccounts {
			configured: vec![key.clone()],
			ready: vec![acct],
		});
		Self {
			mail,
			events,
			key,
			_dir: dir,
		}
	}

	fn wait(&self, what: &str, mut pred: impl FnMut(&Event) -> bool) -> Event {
		let end = Instant::now() + WAIT;
		loop {
			let left = end.saturating_duration_since(Instant::now());
			match self.events.recv_timeout(left) {
				Ok(e) if pred(&e) => return e,
				Ok(_) => {}
				Err(_) => panic!("timed out waiting for {what}"),
			}
		}
	}

	fn page(&self, req: PageRequest) {
		self.mail.send(Command::Page {
			ws: "ws".into(),
			accounts: vec![self.key.clone()],
			req,
		});
	}

	/// The JSON a page script carries, for `window.__mail.<name>(…)`.
	fn page_reply(&self, name: &str) -> serde_json::Value {
		let prefix = format!("window.__mail && window.__mail.{name}(");
		match self.wait(
			name,
			|e| matches!(e, Event::ToPage { script, .. } if script.starts_with(&prefix)),
		) {
			Event::ToPage { script, .. } => serde_json::from_str(&script[prefix.len()..script.len() - 1]).unwrap(),
			_ => unreachable!(),
		}
	}

	fn inbox(&self) -> serde_json::Value {
		self.page(PageRequest::Inbox);
		self.page_reply("inbox")["view"].clone()
	}

	fn act(&self, verb: Verb, thread: &str, until: Option<i64>) {
		self.page(PageRequest::Act {
			verb,
			items: vec![Item {
				account: self.key.clone(),
				thread: thread.into(),
			}],
			until,
		});
		self.page_reply("acted");
	}
}

fn eventually(what: &str, mut check: impl FnMut() -> bool) {
	let end = Instant::now() + WAIT;
	while !check() {
		assert!(Instant::now() < end, "timed out: {what}");
		std::thread::sleep(Duration::from_millis(300));
	}
}

fn now() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_secs() as i64
}

#[test]
#[ignore = "needs the GreenMail container; see the module docs"]
fn mail_e2e() {
	purge("me@local.test");
	purge("anna@local.test");
	let first = deliver(
		"Anna <anna@local.test>",
		"me@local.test",
		"Old news",
		"from before octoweb",
		None,
		false,
	);

	let h = Harness::start(false);
	h.wait("online", |e| matches!(e, Event::Status { state: "online", .. }));
	let view = h.inbox();
	assert_eq!(
		view["important"][0]["thread"], first,
		"first sync brings in existing mail without notifying"
	);

	// New mail arrives through IDLE and notifies (SYN-1, NTF-1).
	let lunch = deliver(
		"Anna <anna@local.test>",
		"me@local.test",
		"Lunch?",
		"Tuesday works?",
		None,
		false,
	);
	let note = h.wait("notify", |e| matches!(e, Event::Notify { .. }));
	let Event::Notify { item, subject, .. } = note else {
		unreachable!()
	};
	assert_eq!(subject, "Lunch?");
	assert_eq!(item.thread, lunch);

	// A newsletter is Other and does not notify (INB-3, NTF-2).
	deliver(
		"Shop <noreply@shop.test>",
		"me@local.test",
		"Sale",
		"50% off",
		None,
		false,
	);
	eventually("newsletter synced", || {
		!h.inbox()["other"].as_array().unwrap().is_empty()
	});
	let view = h.inbox();
	assert_eq!(view["other"][0]["addr"], "noreply@shop.test");
	assert_eq!(view["important"].as_array().unwrap().len(), 2);

	// Opening marks read on the server (READ-5) and returns the body.
	h.page(PageRequest::Thread {
		account: h.key.clone(),
		thread: lunch.clone(),
		peek: false,
	});
	let thread = h.page_reply("thread");
	let message_id = thread["messages"][0]["id"].as_i64().unwrap();
	assert_eq!(thread["messages"][0]["text"].as_str().unwrap().trim(), "Tuesday works?");

	// Done archives on the server; undo brings it back (TRI-1, TRI-2).
	h.act(Verb::Done, &lunch, None);
	eventually("archived on the server", || {
		server("me@local.test", "Archive").iter().any(|(id, _)| *id == lunch)
			&& !server("me@local.test", "INBOX").iter().any(|(id, _)| *id == lunch)
	});
	assert!(h.inbox()["important"]
		.as_array()
		.unwrap()
		.iter()
		.all(|t| t["thread"] != lunch));
	h.act(Verb::Restore, &lunch, None);
	eventually("restored to the inbox", || {
		server("me@local.test", "INBOX").iter().any(|(id, _)| *id == lunch)
	});

	// A notes task made from an email: ticking it when the email is already
	// done keeps it done; unticking brings it back (TRI-8, TRI-10).
	h.act(Verb::ToNotes, &lunch, None);
	eventually("to notes archives", || {
		!server("me@local.test", "INBOX").iter().any(|(id, _)| *id == lunch)
	});
	let item = Item {
		account: h.key.clone(),
		thread: lunch.clone(),
	};
	h.mail.send(Command::TaskToggled {
		item: item.clone(),
		done: true,
	});
	h.mail.send(Command::TaskToggled { item, done: false });
	eventually("unticking restores", || {
		server("me@local.test", "INBOX").iter().any(|(id, _)| *id == lunch)
	});

	// A reply threads in the recipient's client (WRT-1) and is filed in Sent (WRT-8).
	let reply = Draft {
		key: format!("reply:{}:{lunch}", h.key),
		account: h.key.clone(),
		mode: "reply".into(),
		thread: Some(lunch.clone()),
		message: Some(message_id),
		to: "anna@local.test".into(),
		subject: "Re: Lunch?".into(),
		body: "Yes, Tuesday.".into(),
		..Default::default()
	};
	h.page(PageRequest::Send {
		draft: reply,
		send_at: Some(now()),
		remind_after: Some(3600),
	});
	assert_eq!(h.page_reply("queued")["later"], true);
	eventually("reply delivered with In-Reply-To", || {
		server("anna@local.test", "INBOX").iter().any(|(_, irt)| *irt == lunch)
	});
	eventually("reply filed in Sent", || !server("me@local.test", "Sent").is_empty());

	// Undo send keeps a message from leaving (WRT-4).
	let undone = Draft {
		key: "new:undo".into(),
		account: h.key.clone(),
		mode: "new".into(),
		to: "anna@local.test".into(),
		subject: "Never mind".into(),
		body: "oops".into(),
		..Default::default()
	};
	h.page(PageRequest::Send {
		draft: undone,
		send_at: None,
		remind_after: None,
	});
	let queued = h.page_reply("queued");
	h.page(PageRequest::CancelSend {
		id: queued["id"].as_i64().unwrap(),
	});
	assert_eq!(h.page_reply("cancelled")["ok"], true);

	// Mute: later replies skip the inbox and do not notify (TRI-6).
	h.act(Verb::Mute, &lunch, None);
	let follow_up = deliver(
		"Anna <anna@local.test>",
		"me@local.test",
		"Re: Lunch?",
		"Great!",
		Some(&lunch),
		false,
	);
	eventually("muted reply archived on arrival", || {
		server("me@local.test", "Archive")
			.iter()
			.any(|(id, _)| *id == follow_up)
	});
	assert!(h.inbox()["important"]
		.as_array()
		.unwrap()
		.iter()
		.all(|t| t["thread"] != lunch));
	while let Ok(e) = h.events.try_recv() {
		assert!(!matches!(e, Event::Notify { .. }), "a muted thread notified");
	}

	// Snooze leaves the inbox and comes back marked (TRI-5).
	h.act(Verb::Snooze, &first, Some(now() + 2));
	assert!(h.inbox()["important"]
		.as_array()
		.unwrap()
		.iter()
		.all(|t| t["thread"] != first));
	eventually("snooze returned", || {
		h.inbox()["important"]
			.as_array()
			.unwrap()
			.iter()
			.any(|t| t["thread"] == first && t["returned"] == "snooze")
	});

	// Search covers archived mail too (SRCH-1).
	h.page(PageRequest::Search {
		query: "tuesday from:anna".into(),
		server: false,
	});
	let hits = h.page_reply("search")["hits"].clone();
	assert!(hits.as_array().unwrap().iter().any(|t| t["thread"] == lunch));

	// Mail older than the window is found on the server (SYN-2).
	file_old("me@local.test", "Archive", "Ancientinvoice");
	h.page(PageRequest::Search {
		query: "ancientinvoice".into(),
		server: true,
	});
	assert!(
		h.page_reply("search")["hits"].as_array().unwrap().is_empty(),
		"not stored locally yet"
	);
	let hits = h.page_reply("search")["hits"].clone();
	assert_eq!(hits[0]["subject"], "Ancientinvoice", "the server search brings it in");

	// Attachments save to Downloads (READ-4).
	let with_file = deliver(
		"Anna <anna@local.test>",
		"me@local.test",
		"File",
		"see attached",
		None,
		true,
	);
	h.wait(
		"notify file",
		|e| matches!(e, Event::Notify { item, .. } if item.thread == with_file),
	);
	h.page(PageRequest::Thread {
		account: h.key.clone(),
		thread: with_file.clone(),
		peek: false,
	});
	let thread = h.page_reply("thread");
	assert_eq!(thread["messages"][0]["attachments"][0]["name"], "notes.txt");
	h.page(PageRequest::Attachment {
		message: thread["messages"][0]["id"].as_i64().unwrap(),
		index: 0,
	});
	let Event::Saved(path) = h.wait("saved", |e| matches!(e, Event::Saved(_))) else {
		unreachable!()
	};
	assert_eq!(std::fs::read(&path).unwrap(), b"attached!");
	std::fs::remove_file(path).unwrap();

	// Gatekeeper holds senders new since it was turned on (GK-1, GK-2).
	let mut gk = account("me@local.test", true);
	gk.key = h.key.clone();
	h.mail.send(Command::SetAccounts {
		configured: vec![h.key.clone()],
		ready: vec![gk],
	});
	h.wait("online again", |e| matches!(e, Event::Status { state: "online", .. }));
	let stranger = deliver(
		"Eve <eve@else.test>",
		"me@local.test",
		"Hello there",
		"we have not met",
		None,
		false,
	);
	eventually("new sender held", || {
		h.inbox()["new_senders"]
			.as_array()
			.unwrap()
			.iter()
			.any(|t| t["thread"] == stranger)
	});
	h.act(Verb::Accept, &stranger, None);
	assert!(h.inbox()["important"]
		.as_array()
		.unwrap()
		.iter()
		.any(|t| t["thread"] == stranger));

	// Attachments in a draft are sent (WRT-9).
	let with_attachment = Draft {
		key: "new:file".into(),
		account: h.key.clone(),
		mode: "new".into(),
		to: "anna@local.test".into(),
		subject: "Here it is".into(),
		body: "attached".into(),
		attachments: vec![DraftFile {
			name: "a.txt".into(),
			mime: "text/plain".into(),
			data: "aGk=".into(),
		}],
		..Default::default()
	};
	h.page(PageRequest::Send {
		draft: with_attachment,
		send_at: Some(now()),
		remind_after: None,
	});
	h.page_reply("queued");
	eventually("attachment delivered", || server("anna@local.test", "INBOX").len() >= 2);
}
