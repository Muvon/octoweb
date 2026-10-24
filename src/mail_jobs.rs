//! One-shot mail AI jobs (spec AI-1 to AI-6): a specialist with no tools
//! reads mail and answers in text, and the app acts on the answer. Mail comes
//! from anyone, so nothing in it reaches an agent that can browse, read tabs
//! or touch the mailbox — the assistant only ever gets what the user types.
//!
//! The engine (`mail.rs`) builds the prompt and names the job; this runs it on
//! the main thread and reports its text as it grows. One run per kind (per
//! reply for drafts): a new one replaces a run still going.

use crate::agent::{AgentEvent, AgentHandle, AgentSpec};
use crate::mail::{self, Job};

struct Run {
	job: Job,
	handle: AgentHandle,
	response: String,
}

/// What a run said since the last poll: its whole answer so far, and how it
/// ended once it has.
pub struct Progress {
	pub job: Job,
	pub response: String,
	pub ended: Option<Result<(), String>>,
}

#[derive(Default)]
pub struct Jobs {
	runs: Vec<Run>,
}

impl Jobs {
	/// Start `job` on its agent with `prompt`, in place of any run in its slot.
	pub fn start(&mut self, job: Job, prompt: String, wake: impl Fn() + Send + Sync + 'static) -> Result<(), String> {
		self.stop(|j| j.same_slot(&job));
		let handle = AgentHandle::connect(
			AgentSpec::one_shot(job.agent(), crate::workspace::DEFAULT_WORKSPACE_ID, None),
			wake,
		)
		.map_err(|e| format!("The AI could not start: {e}"))?;
		if !handle.send_prompt(prompt, vec![]) {
			return Err("The AI closed before it started.".into());
		}
		self.runs.push(Run {
			job,
			handle,
			response: String::new(),
		});
		Ok(())
	}

	/// Drop the runs whose job matches, ending their sessions.
	pub fn stop(&mut self, matches: impl Fn(&Job) -> bool) {
		self.runs.retain(|r| !matches(&r.job));
	}

	pub fn running(&self, matches: impl Fn(&Job) -> bool) -> bool {
		self.runs.iter().any(|r| matches(&r.job))
	}

	/// Drain every run's events; ended runs are dropped.
	pub fn poll(&mut self) -> Vec<Progress> {
		let mut out = Vec::new();
		self.runs.retain_mut(|run| {
			let mut ended = None;
			let mut grew = false;
			for ev in run.handle.poll() {
				match ev {
					AgentEvent::Chunk(chunk) => {
						run.response.push_str(&chunk);
						grew = true;
					}
					AgentEvent::Done => ended = Some(Ok(())),
					AgentEvent::Error(e) | AgentEvent::TurnError(e) => ended = Some(Err(e)),
					_ => {}
				}
			}
			if grew || ended.is_some() {
				out.push(Progress {
					job: run.job.clone(),
					response: run.response.clone(),
					ended: ended.clone(),
				});
			}
			ended.is_none()
		});
		out
	}
}

/// What the app does with a run's progress.
#[derive(Default)]
pub struct Outcome {
	/// For the mail page.
	pub script: Option<String>,
	/// For the calendar page (spec 002-calendar AI-3, AI-4).
	pub calendar_script: Option<String>,
	/// For the engine.
	pub command: Option<mail::Command>,
	/// For main.rs: what a calendar job's answer asks the app to do.
	pub action: Option<Action>,
	/// For the user, as a toast.
	pub toast: Option<String>,
	/// A reply finished writing: the draft-ready sound.
	pub ready: bool,
}

/// A calendar job's answer, for main.rs to act on: these touch the calendar
/// pane, the notes and the composer, which only the main thread owns.
#[derive(Debug)]
pub enum Action {
	/// AI-1: the quick-add line for the event a conversation of workspace
	/// `ws` arranges; `people` are the only guests it may invite.
	EventLine {
		ws: String,
		line: String,
		people: Vec<String>,
	},
	/// AI-4: tasks for the notes section of meeting `key`, as the reader
	/// wrote them (a deadline as ` >YYYY-MM-DD`).
	MeetingTasks {
		ws: String,
		key: String,
		start: usize,
		heading: String,
		day: String,
		tasks: Vec<String>,
	},
	/// AI-4: the follow-up mail for meeting `key`.
	FollowUp {
		ws: String,
		key: String,
		account: String,
		to: String,
		subject: String,
		body: String,
	},
}

/// The calendar page's busy state for an after-meeting button (AI-4).
pub fn ai_state(key: &str, kind: &str, busy: bool, message: &str) -> String {
	crate::calendar::script(
		"aiState",
		&[serde_json::json!({ "key": key, "kind": kind, "busy": busy, "message": message })],
	)
}

/// How a job that did not finish shows: `message` where the user asked for it.
pub fn failed(job: &Job, message: String) -> Outcome {
	let script = |name: &str, data: serde_json::Value| Outcome {
		script: Some(mail::script(name, &data)),
		..Default::default()
	};
	match job {
		Job::Draft { key } => script("draftFailed", serde_json::json!({ "key": key, "message": message })),
		Job::Summary { item } => script("summaryFailed", serde_json::json!({ "item": item, "message": message })),
		Job::Search { ws, id } => script(
			"searchFailed",
			serde_json::json!({ "ws": ws, "id": id, "message": message }),
		),
		Job::Brief { ws, .. } => {
			tracing::warn!(error = %message, "mail brief run failed");
			script("briefFailed", serde_json::json!({ "ws": ws, "message": message }))
		}
		Job::Tasks { .. } | Job::Event { .. } => Outcome {
			toast: Some(message),
			..Default::default()
		},
		Job::Prep { key } => Outcome {
			calendar_script: Some(crate::calendar::script(
				"prepFailed",
				&[serde_json::json!({ "key": key, "message": message })],
			)),
			..Default::default()
		},
		Job::MeetingTasks { key, .. } => Outcome {
			calendar_script: Some(ai_state(key, "tasks", false, &message)),
			..Default::default()
		},
		Job::FollowUp { key, .. } => Outcome {
			calendar_script: Some(ai_state(key, "followup", false, &message)),
			..Default::default()
		},
		// In the background: nobody is waiting for it.
		Job::Summaries { .. } => {
			tracing::warn!(error = %message, "mail summaries run failed");
			Outcome::default()
		}
	}
}

pub fn outcome(p: Progress) -> Outcome {
	let block = mail::text_block(&p.response);
	let ended = match p.ended {
		Some(Err(e)) => return failed(&p.job, format!("The AI stopped: {e}")),
		Some(Ok(())) => true,
		None => false,
	};
	// Finished without its block: whatever it said instead (a question, a
	// refusal) is the message.
	// An empty answer means "none" for tasks; for an event, that there is no
	// date to add.
	let empty_ok = matches!(p.job, Job::Tasks { .. } | Job::MeetingTasks { .. });
	if ended && block.is_none_or(|(t, _)| t.trim().is_empty() && !empty_ok) {
		if block.is_some() && matches!(p.job, Job::Event { .. }) {
			return failed(&p.job, "No date for an event in this conversation.".into());
		}
		let said: String = p.response.trim().chars().take(300).collect();
		return failed(
			&p.job,
			if said.is_empty() {
				"The AI returned nothing.".into()
			} else {
				said
			},
		);
	}
	let script = |name: &str, data: serde_json::Value| Outcome {
		script: Some(mail::script(name, &data)),
		ready: ended && name == "draftStream",
		..Default::default()
	};
	match (p.job, block) {
		(Job::Draft { key }, Some((text, _))) => script(
			"draftStream",
			serde_json::json!({ "key": key, "text": text, "done": ended }),
		),
		(Job::Summary { item }, Some((text, _))) => script(
			"summaryStream",
			serde_json::json!({ "item": item, "text": text, "done": ended }),
		),
		(Job::Search { ws, id }, Some((text, true))) if ended && text.trim().lines().count() == 1 => script(
			"searchReady",
			serde_json::json!({ "ws": ws, "id": id, "query": text.trim() }),
		),
		(job @ Job::Search { .. }, _) if ended => failed(&job, "The AI did not return a single search query.".into()),
		(Job::Summaries { accounts, items }, _) if ended => {
			let summaries = mail::parse_summaries(&items, &p.response);
			tracing::info!(asked = items.len(), got = summaries.len(), "mail summaries run ended");
			Outcome {
				command: Some(mail::Command::Summaries { accounts, summaries }),
				..Default::default()
			}
		}
		(Job::Tasks { ws, accounts, item }, _) if ended => Outcome {
			command: Some(mail::Command::Tasks {
				ws,
				accounts,
				item,
				tasks: mail::parse_tasks(&p.response),
			}),
			..Default::default()
		},
		(Job::Brief { ws, accounts, items }, _) if ended => {
			let groups = mail::parse_brief(&items, &p.response);
			let sorted: usize = groups.iter().map(|(_, list)| list.len()).sum();
			tracing::info!(asked = items.len(), sorted, "mail brief run ended");
			if sorted == 0 {
				return failed(
					&Job::Brief { ws, accounts, items },
					"The AI did not sort the inbox.".into(),
				);
			}
			Outcome {
				command: Some(mail::Command::Brief { ws, accounts, groups }),
				..Default::default()
			}
		}
		(Job::Prep { key }, Some((text, _))) => Outcome {
			calendar_script: Some(crate::calendar::script(
				"prep",
				&[serde_json::json!({ "key": key, "text": text, "done": ended })],
			)),
			..Default::default()
		},
		(Job::Event { ws, item, people }, _) if ended => match mail::parse_event_line(&p.response) {
			Some(line) => Outcome {
				action: Some(Action::EventLine { ws, line, people }),
				..Default::default()
			},
			None => failed(&Job::Event { ws, item, people }, "No date for an event in this conversation.".into()),
		},
		(
			Job::MeetingTasks {
				ws,
				key,
				start,
				heading,
				day,
			},
			_,
		) if ended => Outcome {
			action: Some(Action::MeetingTasks {
				ws,
				key,
				start,
				heading,
				day,
				tasks: mail::parse_tasks(&p.response),
			}),
			..Default::default()
		},
		(
			Job::FollowUp {
				ws,
				key,
				account,
				to,
				subject,
			},
			Some((text, _)),
		) if ended => Outcome {
			action: Some(Action::FollowUp {
				ws,
				key,
				account,
				to,
				subject,
				body: text.to_string(),
			}),
			..Default::default()
		},
		_ => Outcome::default(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mail::{Command, Item};

	fn item() -> Item {
		Item {
			account: "acc".into(),
			thread: "t".into(),
		}
	}

	fn run(job: Job, response: &str, ended: Option<Result<(), String>>) -> Outcome {
		outcome(Progress {
			job,
			response: response.into(),
			ended,
		})
	}

	#[test]
	fn drafts_for_different_replies_run_side_by_side() {
		let draft = |k: &str| Job::Draft { key: k.into() };
		assert!(draft("reply:a:1").same_slot(&draft("reply:a:1")));
		assert!(!draft("reply:a:1").same_slot(&draft("reply:a:2")));
		let summary = |t: &str| Job::Summary {
			item: Item {
				account: "a".into(),
				thread: t.into(),
			},
		};
		assert!(summary("1").same_slot(&summary("2")));
		assert!(!draft("reply:a:1").same_slot(&summary("1")));
	}

	#[test]
	fn replies_and_summaries_stream_to_the_page() {
		let draft = || Job::Draft { key: "k".into() };
		let o = run(draft(), "<text>\nHi Ann", None);
		assert!(o.script.unwrap().contains("draftStream") && !o.ready);
		let o = run(draft(), "<text>\nHi Ann\n</text>", Some(Ok(())));
		assert!(o.script.unwrap().contains(r#""done":true"#) && o.ready);
		// Done without the block: what it said instead is the error.
		let o = run(draft(), "I can't help with that.", Some(Ok(())));
		assert!(o.script.unwrap().contains("I can't help with that."));
		let o = run(Job::Summary { item: item() }, "<text>\nAnna asks", None);
		assert!(o.script.unwrap().contains("summaryStream"));
		let o = run(Job::Summary { item: item() }, "", Some(Err("gone".into())));
		assert!(o.script.unwrap().contains("summaryFailed"));
		assert!(run(draft(), "thinking", None).script.is_none());
	}

	#[test]
	fn search_waits_for_a_complete_query_and_preserves_its_request() {
		let search = || Job::Search {
			ws: "work".into(),
			id: 7,
		};
		assert_eq!(search().agent(), "octoweb:mail-reader");
		assert!(search().same_slot(&Job::Search {
			ws: "other".into(),
			id: 8
		}));
		assert!(!search().same_slot(&Job::Summary { item: item() }));
		assert!(run(search(), "<text>from:Anna", None).script.is_none());
		assert!(run(search(), "<text>from:Anna</text>", None).script.is_none());
		let o = run(search(), "<text>\nfrom:Anna invoice\n</text>", Some(Ok(())));
		assert_eq!(
			o.script,
			Some(mail::script(
				"searchReady",
				&serde_json::json!({
					"ws": "work", "id": 7, "query": "from:Anna invoice"
				})
			))
		);
		assert!(o.command.is_none() && !o.ready);
		for response in [
			"<text>from:Anna",
			"<text>from:Anna\ninvoice</text>",
			"<text></text>",
			"Sorry.",
		] {
			let o = run(search(), response, Some(Ok(())));
			let script = o.script.unwrap();
			assert!(script.contains("searchFailed"), "{script}");
			assert!(script.contains(r#""ws":"work""#) && script.contains(r#""id":7"#));
			assert!(o.command.is_none());
		}
		let o = run(search(), "", Some(Err("offline".into())));
		assert!(o.script.unwrap().contains("The AI stopped: offline"));
		let o = failed(&search(), "The AI could not start".into());
		assert!(o.script.unwrap().contains("searchFailed"));
	}

	#[test]
	fn the_app_saves_summaries_and_tasks_itself() {
		let summaries = Job::Summaries {
			accounts: vec!["acc".into()],
			items: vec![item()],
		};
		let o = run(summaries, "<text>\n1: Anna asks\n</text>", Some(Ok(())));
		assert!(matches!(o.command, Some(Command::Summaries { summaries, .. }) if summaries.len() == 1));
		let tasks = || Job::Tasks {
			ws: "w".into(),
			accounts: vec!["acc".into()],
			item: item(),
		};
		let o = run(tasks(), "<text>\n- Pay\n</text>", Some(Ok(())));
		assert!(matches!(o.command, Some(Command::Tasks { tasks, .. }) if tasks == ["Pay"]));
		// An empty answer is "no tasks"; no answer at all is an error.
		let o = run(tasks(), "<text>\n</text>", Some(Ok(())));
		assert!(matches!(o.command, Some(Command::Tasks { tasks, .. }) if tasks.is_empty()));
		let o = run(tasks(), "Sorry.", Some(Ok(())));
		assert_eq!(o.toast.as_deref(), Some("Sorry."));
		assert!(o.command.is_none());
	}

	#[test]
	fn calendar_jobs_hand_their_answers_to_the_app() {
		let event = || Job::Event {
			ws: "w".into(),
			item: item(),
			people: vec!["anna@x.com".into()],
		};
		let o = run(event(), "<text>\nReview 2026-10-29 15:00-16:00 anna@x.com\n</text>", Some(Ok(())));
		assert!(matches!(o.action, Some(Action::EventLine { line, people, .. })
			if line == "Review 2026-10-29 15:00-16:00 anna@x.com" && people == ["anna@x.com"]));
		// No event: the user hears why, and no form opens.
		for none in ["<text>\n</text>", "<text>\nNo event found.\n</text>", "<text>\n-\n</text>"] {
			let o = run(event(), none, Some(Ok(())));
			assert!(o.action.is_none(), "{none}");
			assert_eq!(o.toast.as_deref(), Some("No date for an event in this conversation."));
		}
		// One run per conversation: another one's does not stop it.
		let other = Job::Event {
			ws: "w".into(),
			item: Item {
				account: "acc".into(),
				thread: "u".into(),
			},
			people: vec![],
		};
		assert!(event().same_slot(&event()) && !event().same_slot(&other));
		let prep = || Job::Prep { key: "k".into() };
		let o = run(prep(), "<text>\nAnna wants", None);
		assert!(o.calendar_script.unwrap().contains("__cal.prep("));
		let o = run(prep(), "", Some(Err("gone".into())));
		assert!(o.calendar_script.unwrap().contains("prepFailed"));
		let tasks = || Job::MeetingTasks {
			ws: "w".into(),
			key: "k".into(),
			start: 3,
			heading: "### X".into(),
			day: "2026-10-22".into(),
		};
		let o = run(tasks(), "<text>\n- Send deck >2026-10-30\n</text>", Some(Ok(())));
		assert!(matches!(o.action, Some(Action::MeetingTasks { tasks, start: 3, .. }) if tasks == ["Send deck >2026-10-30"]));
		let o = run(tasks(), "<text>\n</text>", Some(Ok(())));
		assert!(matches!(o.action, Some(Action::MeetingTasks { tasks, .. }) if tasks.is_empty()));
		let o = run(tasks(), "", Some(Err("gone".into())));
		assert!(o.calendar_script.unwrap().contains("aiState"));
		let follow = Job::FollowUp {
			ws: "w".into(),
			key: "k".into(),
			account: "a".into(),
			to: "b@x".into(),
			subject: "S".into(),
		};
		assert_eq!(follow.agent(), "octoweb:mail-writer");
		let o = run(follow, "<text>\nHi all,\n</text>", Some(Ok(())));
		assert!(matches!(o.action, Some(Action::FollowUp { body, .. }) if body == "Hi all,"));
	}
}
