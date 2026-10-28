//! Assistant sessions on the shared octomind server.
//!
//! Every chat, background job and inline edit is one octomind session on the
//! single `octomind server` process (`octomind_server.rs`), reached over its
//! WebSocket protocol. A handle owns one connection: it binds its session with
//! the session's working directory and role, streams the session's frames back
//! to the main thread as `AgentEvent`s, and sends prompts, slash commands and
//! cancels.
//!
//! Flow:
//!   1. `AgentHandle::connect()` starts the session task (and the server if
//!      none runs yet), binds the session, and reports `AgentEvent::Connected`.
//!   2. `send_prompt()` runs a turn; frames stream back until its closing
//!      `cost` frame turns into `Done` (or `Cancelled`).
//!   3. The main thread drains `poll()` on every event-loop tick.
//!
//! Turns the agent starts by itself — a specialist's reply, a schedule —
//! stream on the same connection between client turns and end with `Done`.

use crate::specialist;
use anyhow::Context;
use futures::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// Events streamed from the agent back to the main thread.
#[derive(Debug)]
pub enum AgentEvent {
	/// The session is bound and ready. Carries the octomind session id, which
	/// resumes the same conversation on the next connect.
	Connected(String),
	/// A text chunk from the agent's response (streaming).
	Chunk(String),
	/// A delegated specialist transcript; never part of the parent response.
	Run(specialist::Update),
	/// A message injected into the session by the agent runtime — a specialist
	/// (tap-run) reply, schedule, webhook, etc., prefixed with its source label.
	/// Rendered as its own bubble, never merged into the streamed response.
	Injected(String),
	/// A new tool call started (id, title, kind, raw_input, locations).
	ToolStart {
		id: String,
		title: String,
		kind: String,
		raw_input: Option<serde_json::Value>,
		locations: Vec<String>,
	},
	/// An existing tool call was updated (id, optional new title, status, raw_output).
	ToolUpdate {
		id: String,
		title: Option<String>,
		status: String,
		raw_output: Option<serde_json::Value>,
	},
	/// Our prompt's turn finished.
	Done,
	/// A turn the agent started by itself finished (a specialist reply, a
	/// schedule, a handback, or the drain run ahead of a prompt). Not the end
	/// of any prompt: one may still be waiting or running.
	BackgroundDone,
	/// The turn was stopped by `cancel()`.
	Cancelled,
	/// The turn failed (provider error, bad attachment, …). The session and its
	/// connection stay usable — this is not a disconnect.
	TurnError(String),
	/// The connection failed or closed: the handle is dead, reconnect.
	Error(String),
	/// Slash commands the session accepts, for the sidebar's autocomplete.
	AvailableCommands(Vec<CommandInfo>),
	/// Tap workflows the agent can run, as the JSON array from a `/workflow`
	/// command. Feeds the sidebar's `/workflow <name>` autocomplete.
	Workflows(String),
	/// Pending `/schedule list` text. Drives the sidebar's routines chip.
	Schedules(String),
	/// Octomind account status from a `/usage` command. Drives the sidebar's
	/// account card; `signed_in` also ends a pending login flow.
	Account {
		signed_in: bool,
		/// octomind's `/usage` output as sent: account ("email (plan)"), spend
		/// windows, balance, storage and network.
		usage: serde_json::Value,
	},
	/// A device-login flow started (from `/login`). The client opens `url` in a
	/// browser tab, shows `code`, and polls `/usage` until signed in.
	LoginStarted {
		url: String,
		code: String,
		already_signed_in: bool,
	},
}

/// A prompt with optional image attachments.
pub struct PromptMessage {
	pub text: String,
	pub images: Vec<(String, String)>, // (base64_data, mime_type)
}

/// A slash command the session accepts, for the UI.
#[derive(Debug, Clone)]
pub struct CommandInfo {
	pub name: String,
	pub description: String,
	/// Optional input hint (e.g. "describe what to search for").
	pub hint: Option<String>,
}

/// What session a handle drives and where it runs.
pub struct AgentSpec {
	/// Agent tag (`octoweb:assistant`, `octoweb:mail-reader`, …). A resumed
	/// session keeps the role it was created with.
	pub role: String,
	/// The octomind session to resume; `None` starts a new one.
	pub session_id: Option<String>,
	/// Picks the session's working directory (`agent_workspace::prepare`).
	pub workspace_id: String,
	/// The octoweb MCP token whose scope this session's tool calls act in: a
	/// chat's token names its workspace and the chat, so browser tools act on
	/// that workspace's tabs and `render_ui` draws into that chat. `None`
	/// leaves tool calls on the default workspace.
	pub mcp_token: Option<String>,
	/// A one-shot session (a background job): unloaded from the server once
	/// the handle is dropped, instead of kept for a later resume.
	pub one_shot: bool,
}

impl AgentSpec {
	/// A sidebar chat: resumable, kept on the server between connections.
	pub fn chat(role: &str, session_id: Option<String>, workspace_id: &str, mcp_token: Option<String>) -> Self {
		Self {
			role: role.to_string(),
			session_id,
			workspace_id: workspace_id.to_string(),
			mcp_token,
			one_shot: false,
		}
	}

	/// A background job's single-use session.
	pub fn one_shot(role: &str, workspace_id: &str, mcp_token: Option<String>) -> Self {
		Self {
			role: role.to_string(),
			session_id: None,
			workspace_id: workspace_id.to_string(),
			mcp_token,
			one_shot: true,
		}
	}
}

enum Request {
	Prompt(PromptMessage),
	Cancel,
	Command(String),
	RecoverRuns(specialist::Cache),
	/// Unload the session from the server, then disconnect.
	Close,
}

/// Handle passed to the main thread. Send prompts, receive events via `poll`.
/// Dropping it disconnects; the session stays on the server for a resume
/// unless it is one-shot or was `close`d.
pub struct AgentHandle {
	rx: mpsc::UnboundedReceiver<AgentEvent>,
	tx: mpsc::UnboundedSender<Request>,
}

/// Every session connection runs on this one small runtime.
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
	tokio::runtime::Builder::new_multi_thread()
		.worker_threads(2)
		.thread_name("octomind-session")
		.enable_all()
		.build()
		.expect("agent session runtime")
});

impl AgentHandle {
	/// Start the session described by `spec`. Connecting is async — watch
	/// `poll()` for `AgentEvent::Connected` or `AgentEvent::Error`.
	///
	/// `wake` is called whenever an event is pushed — use it to poke the main
	/// event loop out of `ControlFlow::Wait`.
	pub fn connect(spec: AgentSpec, wake: impl Fn() + Send + Sync + 'static) -> anyhow::Result<Self> {
		if spec.role.trim().is_empty() {
			anyhow::bail!("Agent session must name a role");
		}
		let (event_tx, rx) = mpsc::unbounded_channel();
		let (tx, request_rx) = mpsc::unbounded_channel();
		let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
		RUNTIME.spawn(async move {
			let events = Events { tx: event_tx, wake };
			if let Err(e) = run(spec, request_rx, &events).await {
				events.send(AgentEvent::Error(e.to_string()));
			}
		});
		Ok(Self { rx, tx })
	}

	/// Drain all pending events without blocking.
	/// Call this on every event loop tick to forward agent events to the UI.
	pub fn poll(&mut self) -> Vec<AgentEvent> {
		let mut events = Vec::new();
		while let Ok(ev) = self.rx.try_recv() {
			events.push(ev);
		}
		events
	}

	/// Send a user prompt (with optional images) to the agent. A prompt that
	/// starts with `/` runs as that slash command. Returns false if the
	/// session task has exited.
	pub fn send_prompt(&self, text: String, images: Vec<(String, String)>) -> bool {
		self.tx.send(Request::Prompt(PromptMessage { text, images })).is_ok()
	}

	pub fn recover_runs(&self, runs: specialist::Cache) -> bool {
		self.tx.send(Request::RecoverRuns(runs)).is_ok()
	}

	/// Stop the running turn. It ends with `AgentEvent::Cancelled`.
	pub fn cancel(&self) {
		let _ = self.tx.send(Request::Cancel);
	}

	/// Run a slash command (e.g. `/usage`, `/login`) in the background. The
	/// result comes back as an `AgentEvent` (`Account`, `LoginStarted`, …).
	/// Returns false if the session task has exited.
	pub fn send_command(&self, command: String) -> bool {
		self.tx.send(Request::Command(command)).is_ok()
	}

	/// End the session for good: unload it from the server (its log stays on
	/// disk) and disconnect. For a chat the user closed.
	pub fn close(self) {
		let _ = self.tx.send(Request::Close);
	}
}

struct Events {
	tx: mpsc::UnboundedSender<AgentEvent>,
	wake: Arc<dyn Fn() + Send + Sync>,
}

impl Events {
	fn send(&self, event: AgentEvent) {
		let _ = self.tx.send(event);
		(self.wake)();
	}
}

/// One install at a time: sessions connect concurrently and would otherwise
/// race brew for the same formula lock.
static INSTALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Install octomind through Homebrew when it is not on PATH. A DMG install
/// skips the cask, so nothing resolved the dependency for the user. Runs
/// before the server starts: the sidebar just stays in "connecting" while brew
/// works. Best-effort — on failure the start reports the usual "isn't
/// installed" error and the main thread retries as for any disconnect.
pub fn ensure_octomind(program: &str) {
	let missing = || {
		matches!(
			std::process::Command::new(program).arg("--version").output(),
			Err(e) if e.kind() == std::io::ErrorKind::NotFound
		)
	};
	let _guard = INSTALL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
	// Re-check under the lock: another session may have just installed it.
	if !missing() {
		return;
	}
	tracing::info!("octomind not on PATH — installing via Homebrew");
	let status = std::process::Command::new("brew")
		.args(["install", "muvon/tap/octomind"])
		.stdin(std::process::Stdio::null())
		.stdout(std::process::Stdio::null())
		.stderr(std::process::Stdio::null())
		.status();
	match status {
		Ok(s) if s.success() => {
			tracing::info!("octomind installed via Homebrew");
			// Startup registration ran before octomind existed and only warned.
			crate::macos::register_octomind_tap(crate::OCTOMIND_TAP_NAME);
		}
		Ok(s) => tracing::warn!(code = ?s.code(), "brew install octomind failed"),
		Err(e) => tracing::warn!(error = %e, "brew not on PATH — cannot install octomind"),
	}
}

/// A specialist's reply is forwarded verbatim, and models mimic harness
/// markup: one closed its reply with an invented `<system-reminder>` block.
/// It is not the user's to read, so it is cut, unclosed ones to the end.
fn strip_harness_tags(text: &str) -> String {
	const OPEN: &str = "<system-reminder>";
	const CLOSE: &str = "</system-reminder>";
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(at) = rest.find(OPEN) {
		out.push_str(&rest[..at]);
		rest = match rest[at..].find(CLOSE) {
			Some(end) => &rest[at + end + CLOSE.len()..],
			None => "",
		};
	}
	out.push_str(rest);
	out.trim_end().to_string()
}

/// 12 random bytes as hex — the id of a new session or a media file (octomind
/// accepts exactly 24 alphanumeric characters for the latter).
fn random_hex() -> String {
	use std::io::Read;
	let mut buf = [0u8; 12];
	if std::fs::File::open("/dev/urandom")
		.and_then(|mut f| f.read_exact(&mut buf))
		.is_err()
	{
		let nanos = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map(|d| d.as_nanos())
			.unwrap_or_default();
		buf.copy_from_slice(&nanos.to_le_bytes()[..12]);
	}
	buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Slash commands a session accepts over the server protocol, for the
/// sidebar's autocomplete. CLI-only ones (clipboard, screen, file attach) are
/// left out.
fn available_commands() -> Vec<CommandInfo> {
	let cmd = |name: &str, description: &str, hint: Option<&str>| CommandInfo {
		name: name.to_string(),
		description: description.to_string(),
		hint: hint.map(str::to_string),
	};
	vec![
		cmd("help", "Show available commands", None),
		cmd("role", "View or change current role", Some("<role_name>")),
		cmd("model", "View or change current AI model", Some("<provider:model>")),
		cmd("done", "Compress the conversation so far", Some("[instructions]")),
		cmd("info", "Display token and cost breakdown for this session", None),
		cmd(
			"context",
			"Display session context",
			Some("[all|assistant|user|tool|large]"),
		),
		cmd("workflow", "Execute a workflow", Some("<workflow_name> [input]")),
		cmd(
			"mcp",
			"MCP server management",
			Some("[info|list|full|health|dump|validate]"),
		),
		cmd("plan", "Display current supervisor-owned plan", None),
		cmd("prompt", "Manage prompt templates", Some("[template_name]")),
		cmd(
			"report",
			"Generate detailed usage report for this session",
			Some("[day|week|month] [here]"),
		),
		cmd("skill", "List, filter, or toggle skills", Some("[name|pattern|page]")),
		cmd(
			"effort",
			"View or change reasoning effort level",
			Some("[low|medium|high]"),
		),
		cmd(
			"schedule",
			"Schedule a message to be injected at a future time",
			Some("[list|add|remove|edit] [<id>] [when=...] [message=...] [every=...]"),
		),
		cmd(
			"agents",
			"Show running/offloaded agents in this session",
			Some("[session]"),
		),
		cmd("usage", "Show spend and quotas for your Octomind account", None),
		cmd("login", "Sign in to your Octomind account", None),
	]
}

/// `/name arg …` → (`name`, args). `None` for plain text.
fn parse_slash(text: &str) -> Option<(String, Vec<String>)> {
	let rest = text.trim().strip_prefix('/')?;
	let mut words = rest.split_whitespace();
	let name = words.next()?.to_string();
	Some((name, words.map(str::to_string).collect()))
}

/// What a sent frame is, so its ack and replies route right. Acks and every
/// reply to a frame arrive in send order on one connection.
enum Sent {
	/// A user prompt. Kept whole so a "busy" refusal can resend it.
	Prompt { prompt: PromptMessage, media: Vec<PathBuf> },
	/// A slash command the user typed: its output is shown as the reply.
	/// `/done <instructions>` goes on to run those as a turn, ending on `cost`.
	/// Kept as typed, so a "busy" refusal can resend it.
	UserCommand { text: String, ends_on_cost: bool },
	/// A background command (`/usage` refresh, workflow list, login).
	Probe,
}

impl Sent {
	fn is_turn(&self) -> bool {
		!matches!(self, Sent::Probe)
	}
}

/// Octomind refuses a frame for a session whose lock another turn holds — the
/// session's own background turn (a specialist reply, a schedule), or a turn
/// an earlier connection left running. The prompt is resent the moment a
/// `cost` says the lock is free, and on a backing-off timer otherwise.
const BUSY: &str = "is busy processing another request";
const BUSY_RETRY: Duration = Duration::from_secs(2);
const BUSY_RETRY_MAX: Duration = Duration::from_secs(30);
/// A prompt still refused after this long is given up on, with a message.
const BUSY_GIVE_UP: Duration = Duration::from_secs(30 * 60);
/// A turn that sends nothing for this long is considered stuck. Long tool
/// calls report progress, which counts as activity; a `render_ui` waiting on
/// the user reports nothing and is exempt (it has its own cap in `mcp.rs`).
const IDLE_TIMEOUT: Duration = Duration::from_secs(1200);
/// After `cancel()`, how long the turn gets to close before it is reported
/// cancelled anyway.
const CANCEL_GRACE: Duration = Duration::from_secs(10);
/// Account/quota refresh while the session is connected.
const USAGE_REFRESH: Duration = Duration::from_secs(60);

type WsStream = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// One session's connection state.
struct Session<'a, S> {
	events: &'a Events,
	sink: S,
	session_id: String,
	next_id: u64,
	/// Sent, awaiting their ack, by request id.
	pending: HashMap<String, Sent>,
	/// The acknowledged frame whose replies are arriving now, and its request id.
	current: Option<Sent>,
	current_id: Option<String>,
	/// The server echoes a request's id on the cost that closes its turn
	/// (`session_control_v1`), so turn ends are matched, not inferred.
	tagged_costs: bool,
	run_events: bool,
	runs: specialist::Streams,
	/// Prompts waiting for the running one to finish, or for a busy retry.
	queue: VecDeque<PromptMessage>,
	busy_retries: u32,
	busy_retry_at: Option<Instant>,
	/// When the prompt at the head of the queue was first refused as busy.
	busy_since: Option<Instant>,
	/// An `injected` frame opened a turn the agent started itself; its `cost`
	/// closes it (inside a prompt, before the prompt's own turn runs).
	injected_open: bool,
	/// A failed turn may still send its closing `cost`; it closes nothing.
	swallow_cost: bool,
	cancel_deadline: Option<Instant>,
	last_frame: Instant,
	/// Open `render_ui` calls, by tool id: the turn is waiting on a click,
	/// not stuck, so the idle timeout does not apply.
	asking_user: HashSet<String>,
}

/// Start (or reuse) the server, then serve the session on it.
async fn run(spec: AgentSpec, requests: mpsc::UnboundedReceiver<Request>, events: &Events) -> anyhow::Result<()> {
	let endpoint = tokio::task::spawn_blocking(crate::octomind_server::endpoint).await??;
	let cwd = crate::agent_workspace::prepare(&spec.workspace_id, &spec.role)?;
	serve_session(&endpoint, &cwd, spec, requests, events).await
}

/// Connect, bind, and serve the session until the handle goes away or the
/// connection fails.
async fn serve_session(
	endpoint: &crate::octomind_server::Endpoint,
	cwd: &std::path::Path,
	spec: AgentSpec,
	mut requests: mpsc::UnboundedReceiver<Request>,
	events: &Events,
) -> anyhow::Result<()> {
	let session_id = spec
		.session_id
		.clone()
		.unwrap_or_else(|| format!("octoweb-{}", random_hex()));
	// Route this session's tool calls before it can make any.
	if let Some(token) = &spec.mcp_token {
		crate::mcp::route_agent_session(&session_id, token);
		// Specialists it delegates to run as their own sessions in this
		// directory; they reach the workspace's browser through it.
		crate::mcp::route_agent_workdir(cwd, &spec.workspace_id);
	}

	let mut request = endpoint.url.as_str().into_client_request()?;
	request
		.headers_mut()
		.insert("Authorization", format!("Bearer {}", endpoint.token).parse()?);
	let (ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(request))
		.await
		.map_err(|_| anyhow::anyhow!("timed out connecting to the octomind server"))??;
	let (sink, mut stream) = ws.split();

	let mut session = Session {
		events,
		sink,
		session_id: session_id.clone(),
		next_id: 0,
		pending: HashMap::new(),
		current: None,
		current_id: None,
		tagged_costs: false,
		run_events: false,
		runs: specialist::Streams::default(),
		queue: VecDeque::new(),
		busy_retries: 0,
		busy_retry_at: None,
		busy_since: None,
		injected_open: false,
		swallow_cost: false,
		cancel_deadline: None,
		last_frame: Instant::now(),
		asking_user: HashSet::new(),
	};

	// A session that failed to come up may still be loading server-side; a
	// one-shot must be closed below or it holds a slot of the server's session cap.
	let started = async {
		session.bind(&mut stream, cwd, &spec.role).await?;
		if spec.one_shot {
			session.recover_runs(specialist::Cache::default()).await?;
		}
		anyhow::Ok(())
	}
	.await;
	let result = match started {
		Ok(()) => {
			events.send(AgentEvent::Connected(session_id.clone()));
			events.send(AgentEvent::AvailableCommands(available_commands()));
			session.serve(&mut stream, &mut requests).await
		}
		Err(e) => Err(e),
	};

	// Leaving mid-turn: stop the turn rather than leave it holding the session
	// against the next connection (a reconnect would only hear "busy").
	let turn_running = session.current.as_ref().is_some_and(Sent::is_turn)
		|| session.pending.values().any(Sent::is_turn)
		|| session.injected_open;
	if result.is_ok() && turn_running {
		let _ = session.send_cancel().await;
	}
	if spec.one_shot {
		crate::mcp::unroute_agent_session(&session_id);
	}
	if spec.one_shot || matches!(result, Ok(Ending::Closed)) {
		let _ = session
			.send_frame(serde_json::json!({
				"type": "close", "session_id": session_id
			}))
			.await;
		// Let the close land before the socket goes.
		let _ = tokio::time::timeout(Duration::from_secs(2), async {
			while let Some(Ok(frame)) = stream.next().await {
				if let Message::Text(text) = frame {
					if text.contains("\"command_type\":\"close\"") {
						break;
					}
				}
			}
		})
		.await;
	}
	let _ = session.sink.close().await;
	result.map(|_| ())
}

enum Ending {
	/// The handle was dropped.
	Dropped,
	/// The handle asked to end the session.
	Closed,
}

impl<S> Session<'_, S>
where
	S: futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
	fn request_id(&mut self, prefix: &str) -> String {
		self.next_id += 1;
		format!("{prefix}{}", self.next_id)
	}

	async fn send_frame(&mut self, frame: serde_json::Value) -> anyhow::Result<()> {
		self.sink.send(Message::text(frame.to_string())).await?;
		Ok(())
	}

	/// Bind the session (create or resume it under its id) and wait until the
	/// server says it is ready.
	async fn bind(
		&mut self,
		stream: &mut (impl futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
		cwd: &std::path::Path,
		role: &str,
	) -> anyhow::Result<()> {
		self.send_frame(serde_json::json!({
			"type": "session",
			"request_id": "bind",
			"session_id": self.session_id,
			"cwd": cwd,
			"role": role,
		}))
		.await?;
		// Resolving a role the server has not loaded yet may fetch its tap.
		let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
		loop {
			let frame = tokio::time::timeout_at(deadline, stream.next())
				.await
				.map_err(|_| anyhow::anyhow!("timed out starting the agent session"))?
				.ok_or_else(|| anyhow::anyhow!("the octomind server closed the connection"))??;
			let Message::Text(text) = frame else { continue };
			let value: serde_json::Value = serde_json::from_str(&text)?;
			match value["type"].as_str() {
				Some("status")
					if value["session_id"].as_str() == Some(self.session_id.as_str())
						&& value.get("data").is_none() =>
				{
					return Ok(());
				}
				Some("error") => {
					anyhow::bail!(
						"{}",
						value["message"].as_str().unwrap_or("the agent session failed to start")
					);
				}
				Some("ack") if value["request_id"] == "bind" => {
					self.run_events = value["capabilities"]
						.as_array()
						.is_some_and(|caps| caps.iter().any(|c| c == "run_events_v1"));
					self.tagged_costs = value["capabilities"]
						.as_array()
						.is_some_and(|caps| caps.iter().any(|c| c == "session_control_v1"));
				}
				Some("run_event") => self.on_frame(value).await?,
				// The greeting.
				_ => {}
			}
		}
	}

	async fn serve(
		&mut self,
		stream: &mut futures::stream::SplitStream<WsStream>,
		requests: &mut mpsc::UnboundedReceiver<Request>,
	) -> anyhow::Result<Ending> {
		// Account status shows on connect, then stays fresh.
		self.send_probe("/usage").await?;
		let mut usage = tokio::time::interval_at(tokio::time::Instant::now() + USAGE_REFRESH, USAGE_REFRESH);
		let mut tick = tokio::time::interval(Duration::from_secs(1));

		loop {
			tokio::select! {
				request = requests.recv() => match request {
					Some(Request::Prompt(prompt)) => {
						self.queue.push_back(prompt);
						self.send_next_prompt().await?;
					}
					Some(Request::Command(command)) => self.send_probe(&command).await?,
					Some(Request::RecoverRuns(runs)) => self.recover_runs(runs).await?,
					Some(Request::Cancel) => self.cancel().await?,
					Some(Request::Close) => return Ok(Ending::Closed),
					None => return Ok(Ending::Dropped),
				},
				frame = stream.next() => match frame {
					Some(Ok(Message::Text(text))) => {
						match serde_json::from_str::<serde_json::Value>(&text) {
							Ok(value) => self.on_frame(value).await?,
							Err(e) => tracing::debug!(error = %e, "unparseable octomind frame"),
						}
					}
					Some(Ok(Message::Close(_))) | None => {
						anyhow::bail!("the octomind server closed the connection")
					}
					Some(Ok(_)) => {}
					Some(Err(e)) => anyhow::bail!("octomind connection failed: {e}"),
				},
				_ = usage.tick() => self.send_probe("/usage").await?,
				_ = tick.tick() => self.on_tick().await?,
			}
		}
	}

	/// Timers: busy retry, cancel grace, stuck-turn detection.
	async fn on_tick(&mut self) -> anyhow::Result<()> {
		let now = Instant::now();
		if self.busy_retry_at.is_some_and(|at| now >= at) {
			self.busy_retry_at = None;
			self.send_next_prompt().await?;
		}
		if self.cancel_deadline.is_some_and(|at| now >= at) {
			self.cancel_deadline = None;
			if self.current.as_ref().is_some_and(Sent::is_turn) {
				// The turn ignored the stop. Report it stopped; its late cost
				// closes nothing.
				self.end_turn();
				self.swallow_cost = true;
				self.events.send(AgentEvent::Cancelled);
				self.send_next_prompt().await?;
			}
		}
		let waiting = self.current.as_ref().is_some_and(Sent::is_turn) || self.pending.values().any(Sent::is_turn);
		if waiting && self.asking_user.is_empty() && now.duration_since(self.last_frame) > IDLE_TIMEOUT {
			anyhow::bail!("agent idle for 20 minutes — no response");
		}
		Ok(())
	}

	/// Send the next queued prompt, unless a turn is already on its way.
	async fn send_next_prompt(&mut self) -> anyhow::Result<()> {
		let busy = self.current.as_ref().is_some_and(Sent::is_turn)
			|| self.pending.values().any(Sent::is_turn)
			|| self.busy_retry_at.is_some();
		if busy {
			return Ok(());
		}
		let Some(prompt) = self.queue.pop_front() else {
			return Ok(());
		};
		let request_id = self.request_id("p");

		if let Some((name, args)) = parse_slash(&prompt.text) {
			let ends_on_cost = name == "done" && !args.is_empty();
			self.send_frame(serde_json::json!({
				"type": "command",
				"request_id": request_id,
				"session_id": self.session_id,
				"command": name,
				"args": args,
			}))
			.await?;
			self.pending.insert(
				request_id,
				Sent::UserCommand {
					text: prompt.text,
					ends_on_cost,
				},
			);
			return Ok(());
		}

		let (attachments, media) = match write_media(&prompt.images) {
			Ok(written) => written,
			Err(e) => {
				self.events
					.send(AgentEvent::TurnError(format!("could not attach the image: {e}")));
				return Box::pin(self.send_next_prompt()).await;
			}
		};
		let mut frame = serde_json::json!({
			"type": "message",
			"request_id": request_id,
			"session_id": self.session_id,
			"content": prompt.text,
		});
		if !attachments.is_empty() {
			frame["attachments"] = serde_json::Value::Array(attachments);
		}
		self.send_frame(frame).await?;
		self.pending.insert(request_id, Sent::Prompt { prompt, media });
		Ok(())
	}

	async fn send_probe(&mut self, command: &str) -> anyhow::Result<()> {
		let Some((name, args)) = parse_slash(command) else {
			return Ok(());
		};
		let request_id = self.request_id("q");
		self.send_frame(serde_json::json!({
			"type": "command",
			"request_id": request_id,
			"session_id": self.session_id,
			"command": name,
			"args": args,
		}))
		.await?;
		self.pending.insert(request_id, Sent::Probe);
		Ok(())
	}

	/// Stop the running turn — the one we sent, or one the agent started.
	async fn cancel(&mut self) -> anyhow::Result<()> {
		let turn_running = self.current.as_ref().is_some_and(Sent::is_turn)
			|| self.pending.values().any(Sent::is_turn)
			|| self.injected_open;
		// A prompt still waiting (behind a busy session) never reaches the agent.
		let dropped_waiting = !self.queue.is_empty() || self.busy_retry_at.is_some();
		self.queue.clear();
		self.clear_busy();
		if !turn_running {
			if dropped_waiting {
				self.events.send(AgentEvent::Cancelled);
			}
			return Ok(());
		}
		self.send_cancel().await
	}

	async fn send_cancel(&mut self) -> anyhow::Result<()> {
		let request_id = self.request_id("x");
		self.send_frame(serde_json::json!({
			"type": "cancel", "request_id": request_id, "session_id": self.session_id
		}))
		.await?;
		if self.cancel_deadline.is_none() {
			self.cancel_deadline = Some(Instant::now() + CANCEL_GRACE);
		}
		Ok(())
	}

	/// The running turn is over: drop its media and forget it.
	fn end_turn(&mut self) {
		if let Some(Sent::Prompt { media, .. }) = self.current.take() {
			for path in media {
				let _ = std::fs::remove_file(path);
			}
		}
		self.cancel_deadline = None;
		self.injected_open = false;
		self.asking_user.clear();
	}

	async fn recover_runs(&mut self, runs: specialist::Cache) -> anyhow::Result<()> {
		let updates = self.runs.recover(runs);
		if self.run_events {
			for update in updates {
				self.events.send(AgentEvent::Run(update));
			}
		}
		self.request_run_logs().await
	}

	async fn request_run_logs(&mut self) -> anyhow::Result<()> {
		if !self.run_events {
			return Ok(());
		}
		for run_id in self.runs.requests() {
			let request_id = self.request_id("run-log-");
			self.send_frame(serde_json::json!({
				"type": "run_log", "session_id": self.session_id,
				"run_id": run_id, "request_id": request_id,
			}))
			.await?;
			self.runs.request(&run_id, request_id);
		}
		Ok(())
	}

	async fn on_frame(&mut self, value: serde_json::Value) -> anyhow::Result<()> {
		let kind = value["type"].as_str().unwrap_or_default();
		// Run traffic is out of band, including replay acknowledgements. It
		// must not consume any parent turn bookkeeping, even swallow_cost.
		if matches!(kind, "run_event" | "run_log") {
			anyhow::ensure!(self.run_events, "received {kind} without run_events_v1");
			let run = if kind == "run_event" {
				let frame: specialist::Frame = serde_json::from_value(value).context("invalid run_event")?;
				anyhow::ensure!(frame.session_id == self.session_id, "run_event session_id mismatch");
				self.runs.event(frame)
			} else {
				let log: specialist::Log = serde_json::from_value(value).context("invalid run_log")?;
				anyhow::ensure!(log.session_id == self.session_id, "run_log session_id mismatch");
				self.runs.log(log)?
			};
			for update in run {
				self.events.send(AgentEvent::Run(update));
			}
			return self.request_run_logs().await;
		}
		if kind == "ack" && value["message_type"] == "run_log" {
			return Ok(());
		}
		self.last_frame = Instant::now();
		// A failed turn's trailing cost comes right behind its error, before
		// anything else; a cost after any other frame belongs to something else.
		if kind != "cost" {
			self.swallow_cost = false;
		}
		match kind {
			"ack" => {
				if let Some((id, sent)) = value["request_id"]
					.as_str()
					.and_then(|id| self.pending.remove_entry(id))
				{
					let is_turn = sent.is_turn();
					self.current = Some(sent);
					self.current_id = Some(id);
					// A cancel sent while this turn still waited behind another
					// frame found nothing running; the turn starts now.
					if is_turn && self.cancel_deadline.is_some() {
						self.send_cancel().await?;
					}
				}
			}
			"assistant" => {
				if let Some(text) = value["content"].as_str().filter(|t| !t.is_empty()) {
					self.events.send(AgentEvent::Chunk(text.to_string()));
				}
			}
			"tool_use" => {
				let id = value["tool_id"].as_str().unwrap_or_default().to_string();
				let title = value["tool"].as_str().unwrap_or_default().to_string();
				if title.ends_with("render_ui") {
					self.asking_user.insert(id.clone());
				}
				self.events.send(AgentEvent::ToolStart {
					id,
					title,
					kind: "other".to_string(),
					raw_input: value.get("params").cloned(),
					locations: Vec::new(),
				});
			}
			"tool_result" => {
				if let Some(id) = value["tool_id"].as_str() {
					self.asking_user.remove(id);
				}
				let content = value["content"].as_str().unwrap_or_default();
				self.events.send(AgentEvent::ToolUpdate {
					id: value["tool_id"].as_str().unwrap_or_default().to_string(),
					// Restores the title a progress message replaced meanwhile.
					title: value["tool"].as_str().map(str::to_string),
					status: if value["success"].as_bool().unwrap_or(false) {
						"completed"
					} else {
						"failed"
					}
					.to_string(),
					raw_output: Some(
						serde_json::from_str(content)
							.unwrap_or_else(|_| serde_json::Value::String(content.to_string())),
					),
				});
			}
			// Liveness of a running tool call, shown as its title.
			"mcp_notification" => {
				if let (Some(id), Some(message)) = (value["tool_id"].as_str(), value["params"]["message"].as_str()) {
					self.events.send(AgentEvent::ToolUpdate {
						id: id.to_string(),
						title: Some(format!(
							"[{}] {}",
							value["server"].as_str().unwrap_or_default(),
							message
						)),
						status: String::new(),
						raw_output: None,
					});
				}
			}
			"injected" => {
				let text = strip_harness_tags(&format!(
					"[{}] {}",
					value["source_label"].as_str().unwrap_or_default(),
					value["content"].as_str().unwrap_or_default()
				));
				self.injected_open = true;
				if !text.is_empty() {
					self.events.send(AgentEvent::Injected(text));
				}
			}
			"status" => self.on_status(&value).await?,
			"cost" => self.on_cost(&value).await?,
			"error" => {
				let message = value["message"].as_str().unwrap_or("agent error").to_string();
				self.on_error(message).await?;
			}
			// thinking, skill, evolution: nothing to show.
			_ => {}
		}
		Ok(())
	}

	async fn on_status(&mut self, value: &serde_json::Value) -> anyhow::Result<()> {
		let Some(data) = value.get("data") else {
			// "Stopped: … spending threshold reached" — the turn's cost follows.
			if let Some(message) = value["message"].as_str() {
				if message.starts_with("Stopped:") {
					self.events.send(AgentEvent::Chunk(format!("\n\n_{message}._")));
				}
			}
			return Ok(());
		};
		let command_type = data["command_type"].as_str().unwrap_or_default();
		match self.current.take() {
			Some(Sent::UserCommand { ends_on_cost, text }) => {
				self.events.send(AgentEvent::Chunk(render_command_output(data)));
				if ends_on_cost {
					// `/done <instructions>` runs those next, in the same turn.
					self.current = Some(Sent::UserCommand { text, ends_on_cost });
				} else {
					self.events.send(AgentEvent::Done);
					self.send_next_prompt().await?;
				}
				self.forward_command_output(command_type, data);
			}
			other => {
				self.current = other;
				self.forward_command_output(command_type, data);
				if matches!(self.current, Some(Sent::Probe)) {
					self.current = None;
				}
			}
		}
		Ok(())
	}

	/// Structured command results the UI consumes on their own.
	fn forward_command_output(&self, command_type: &str, out: &serde_json::Value) {
		match command_type {
			"usage" => self.events.send(AgentEvent::Account {
				signed_in: out["signed_in"].as_bool().unwrap_or(false),
				usage: out.clone(),
			}),
			"workflow" => {
				if let Some(list) = out.get("data").and_then(|d| d.get("workflows")) {
					self.events.send(AgentEvent::Workflows(list.to_string()));
				}
			}
			"schedule" => {
				if let Some(message) = out["data"]["message"].as_str() {
					self.events.send(AgentEvent::Schedules(message.to_string()));
				}
			}
			"login" => self.events.send(AgentEvent::LoginStarted {
				url: out["verification_url"].as_str().unwrap_or_default().to_string(),
				code: out["user_code"].as_str().unwrap_or_default().to_string(),
				already_signed_in: out["already_signed_in"].as_bool().unwrap_or(false),
			}),
			_ => {}
		}
	}

	async fn on_cost(&mut self, value: &serde_json::Value) -> anyhow::Result<()> {
		if std::mem::take(&mut self.swallow_cost) {
			return Ok(());
		}
		let ours_running = self.current.as_ref().is_some_and(Sent::is_turn);
		let closes_ours = ours_running
			&& if self.tagged_costs {
				value["request_id"].as_str().is_some() && value["request_id"].as_str() == self.current_id.as_deref()
			} else {
				!self.injected_open
			};
		if !closes_ours {
			// A turn the agent started itself is over — a specialist reply, a
			// schedule, a handback, or the drain run ahead of our prompt. It is
			// not our prompt's end: that one, if any, is still waiting or running.
			self.injected_open = false;
			let ours = ours_running;
			if !ours {
				self.cancel_deadline = None;
			}
			self.events.send(AgentEvent::BackgroundDone);
			if !ours {
				// The session's lock is free again: a prompt refused as busy goes now.
				self.busy_retry_at = None;
				self.send_next_prompt().await?;
			}
			return Ok(());
		}
		let cancelled = value["cancelled"].as_bool().unwrap_or(false) || self.cancel_deadline.is_some();
		self.end_turn();
		self.clear_busy();
		self.events.send(if cancelled {
			AgentEvent::Cancelled
		} else {
			AgentEvent::Done
		});
		self.send_next_prompt().await
	}

	/// Forget a busy-retry cycle: the prompt ran, failed, or was cancelled.
	fn clear_busy(&mut self) {
		self.busy_retries = 0;
		self.busy_since = None;
		self.busy_retry_at = None;
	}

	/// Put a refused prompt back at the head of the queue and schedule its
	/// retry, or give up once the session has stayed busy too long.
	async fn retry_busy(&mut self, prompt: PromptMessage) -> anyhow::Result<()> {
		let since = *self.busy_since.get_or_insert_with(Instant::now);
		if since.elapsed() > BUSY_GIVE_UP {
			self.clear_busy();
			self.events.send(AgentEvent::TurnError(
				"The assistant is still busy with an earlier task — try again shortly.".into(),
			));
			return self.send_next_prompt().await;
		}
		let wait = BUSY_RETRY
			.saturating_mul(1 << self.busy_retries.min(4))
			.min(BUSY_RETRY_MAX);
		self.busy_retries += 1;
		self.queue.push_front(prompt);
		self.busy_retry_at = Some(Instant::now() + wait);
		Ok(())
	}

	async fn on_error(&mut self, message: String) -> anyhow::Result<()> {
		if message.contains(BUSY) {
			match self.current.take() {
				Some(Sent::Prompt { prompt, media }) => {
					for path in media {
						let _ = std::fs::remove_file(path);
					}
					self.retry_busy(prompt).await?;
				}
				Some(Sent::UserCommand { text, .. }) => {
					self.retry_busy(PromptMessage {
						text,
						images: Vec::new(),
					})
					.await?;
				}
				// A background probe will run again on its own schedule.
				_ => {}
			}
			return Ok(());
		}
		// A background turn the agent started failed; its cost still follows.
		if message.starts_with("Error processing injected message") {
			self.events.send(AgentEvent::Chunk(format!("\n\n_⚠ {message}_")));
			return Ok(());
		}
		match &self.current {
			Some(Sent::Prompt { .. }) => {
				self.end_turn();
				self.clear_busy();
				// A provider failure still sends the turn's cost right behind.
				self.swallow_cost = true;
				self.events.send(AgentEvent::TurnError(message));
				self.send_next_prompt().await?;
			}
			Some(Sent::UserCommand { .. }) => {
				self.end_turn();
				self.events.send(AgentEvent::Chunk(message));
				self.events.send(AgentEvent::Done);
				self.send_next_prompt().await?;
			}
			Some(Sent::Probe) => {
				self.current = None;
				tracing::debug!(error = %message, "background command failed");
			}
			None => self.events.send(AgentEvent::TurnError(message)),
		}
		Ok(())
	}
}

/// How a typed slash command's result reads in the chat.
fn render_command_output(data: &serde_json::Value) -> String {
	let only_type = data
		.as_object()
		.is_some_and(|o| o.len() == 1 && o.contains_key("command_type"));
	if only_type {
		return "Command executed.".to_string();
	}
	if let Some(message) = data["message"].as_str() {
		if data.as_object().is_some_and(|o| o.len() <= 2) {
			return message.to_string();
		}
	}
	serde_json::to_string_pretty(data).unwrap_or_else(|_| "Command executed.".to_string())
}

/// Write a prompt's images to the server's media store; returns the
/// attachment entries and the files to remove once the turn ends.
fn write_media(images: &[(String, String)]) -> anyhow::Result<(Vec<serde_json::Value>, Vec<PathBuf>)> {
	use base64::Engine;
	let mut attachments = Vec::new();
	let mut paths = Vec::new();
	if images.is_empty() {
		return Ok((attachments, paths));
	}
	let dir = crate::octomind_server::media_dir();
	std::fs::create_dir_all(&dir)?;
	for (data, mime) in images {
		let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;
		let id = random_hex();
		let ext = match mime.as_str() {
			"image/jpeg" | "image/jpg" => "jpg",
			"image/gif" => "gif",
			"image/webp" => "webp",
			_ => "png",
		};
		let name = format!("{id}.{ext}");
		let path = dir.join(&name);
		std::fs::write(&path, &bytes)?;
		attachments.push(serde_json::json!({
			"id": id,
			"kind": "image",
			"media_type": mime,
			"name": name,
			"size": bytes.len(),
		}));
		paths.push(path);
	}
	Ok((attachments, paths))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn injected_replies_lose_invented_harness_tags() {
		let reply = "Draft saved.\n<system-reminder>Context instructions:\n# Reminder\n</system-reminder>";
		assert_eq!(strip_harness_tags(reply), "Draft saved.");
		assert_eq!(strip_harness_tags("a <system-reminder>x</system-reminder> b"), "a  b");
		assert_eq!(strip_harness_tags("a\n<system-reminder>cut off"), "a");
		assert_eq!(strip_harness_tags("<system-reminder>only</system-reminder>"), "");
	}

	#[test]
	fn slash_prompts_split_into_command_and_args() {
		assert_eq!(parse_slash("hello"), None);
		assert_eq!(parse_slash("/"), None);
		assert_eq!(parse_slash("/usage"), Some(("usage".into(), vec![])));
		assert_eq!(
			parse_slash("  /schedule list  now "),
			Some(("schedule".into(), vec!["list".into(), "now".into()]))
		);
	}

	#[test]
	fn media_ids_are_what_octomind_accepts() {
		let id = random_hex();
		assert_eq!(id.len(), 24);
		assert!(id.bytes().all(|b| b.is_ascii_alphanumeric()));
		assert_ne!(id, random_hex());
	}

	// ── The session against a scripted stand-in for `octomind server` ──

	type ServerWs = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

	const TOKEN: &str = "test-token";

	/// Accepts one connection (refusing a wrong token, as the real server
	/// does) and hands it to `script`.
	async fn mock_server<F, Fut>(script: F) -> crate::octomind_server::Endpoint
	where
		F: FnOnce(ServerWs) -> Fut + Send + 'static,
		Fut: std::future::Future<Output = ()> + Send,
	{
		use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let addr = listener.local_addr().unwrap();
		tokio::spawn(async move {
			let (stream, _) = listener.accept().await.unwrap();
			// tungstenite's `Callback` fixes this signature, `ErrorResponse` included.
			#[allow(clippy::result_large_err)]
			let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
				let auth = req.headers().get("authorization").and_then(|v| v.to_str().ok());
				assert_eq!(auth, Some(format!("Bearer {TOKEN}").as_str()));
				Ok(resp)
			};
			let ws = tokio_tungstenite::accept_hdr_async(stream, check).await.unwrap();
			script(ws).await;
		});
		crate::octomind_server::Endpoint {
			url: format!("ws://{addr}/"),
			token: TOKEN.to_string(),
		}
	}

	async fn recv(ws: &mut ServerWs) -> serde_json::Value {
		loop {
			let frame = tokio::time::timeout(Duration::from_secs(10), ws.next())
				.await
				.expect("client frame in time")
				.expect("client still connected")
				.expect("frame decodes");
			if let Message::Text(text) = frame {
				return serde_json::from_str(&text).unwrap();
			}
		}
	}

	async fn send(ws: &mut ServerWs, frame: serde_json::Value) {
		ws.send(Message::text(frame.to_string())).await.unwrap();
	}

	async fn ack(ws: &mut ServerWs, of: &serde_json::Value) {
		send(
			ws,
			serde_json::json!({
				"type": "ack", "request_id": of["request_id"], "message_type": of["type"],
				"session_id": of["session_id"], "status": "received"
			}),
		)
		.await;
	}

	fn cost(session: &str, extra: serde_json::Value) -> serde_json::Value {
		let mut frame = serde_json::json!({
			"type": "cost", "session_tokens": 1, "session_cost": 0.0, "input_tokens": 1,
			"output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0,
			"reasoning_tokens": 0, "session_id": session
		});
		if let serde_json::Value::Object(extra) = extra {
			frame.as_object_mut().unwrap().extend(extra);
		}
		frame
	}

	/// Greeting, bind and the connect-time `/usage` probe. Returns the bind.
	async fn accept_session(ws: &mut ServerWs) -> serde_json::Value {
		accept_session_with_capabilities(ws, &[]).await
	}

	async fn accept_session_with_capabilities(ws: &mut ServerWs, capabilities: &[&str]) -> serde_json::Value {
		send(ws, serde_json::json!({"type": "status", "message": "Connected"})).await;
		let bind = recv(ws).await;
		assert_eq!(bind["type"], "session", "{bind}");
		send(
			ws,
			serde_json::json!({"type":"ack", "request_id":"bind", "message_type":"session", "capabilities":capabilities}),
		)
		.await;
		let sid = bind["session_id"].clone();
		send(
			ws,
			serde_json::json!({"type": "status", "message": "Session created", "session_id": sid}),
		)
		.await;
		let probe = recv(ws).await;
		assert_eq!(probe["command"], "usage", "{probe}");
		ack(ws, &probe).await;
		send(
			ws,
			serde_json::json!({
				"type": "status", "message": "ok", "session_id": sid,
				"data": {"command_type": "usage", "signed_in": true}
			}),
		)
		.await;
		bind
	}

	struct Client {
		requests: mpsc::UnboundedSender<Request>,
		events: mpsc::UnboundedReceiver<AgentEvent>,
		done: tokio::task::JoinHandle<anyhow::Result<()>>,
	}

	fn start(endpoint: crate::octomind_server::Endpoint, spec: AgentSpec) -> Client {
		start_with_runs(endpoint, spec, specialist::Cache::default())
	}

	fn start_with_runs(endpoint: crate::octomind_server::Endpoint, spec: AgentSpec, runs: specialist::Cache) -> Client {
		let (event_tx, events) = mpsc::unbounded_channel();
		let (requests, request_rx) = mpsc::unbounded_channel();
		if !spec.one_shot {
			requests.send(Request::RecoverRuns(runs)).unwrap();
		}
		let done = tokio::spawn(async move {
			let events = Events {
				tx: event_tx,
				wake: Arc::new(|| {}),
			};
			serve_session(&endpoint, std::path::Path::new("/tmp"), spec, request_rx, &events).await
		});
		Client { requests, events, done }
	}

	impl Client {
		fn prompt(&self, text: &str) {
			self.requests
				.send(Request::Prompt(PromptMessage {
					text: text.to_string(),
					images: vec![],
				}))
				.unwrap();
		}

		/// The next event, skipping the connect-time command list and account.
		async fn next(&mut self) -> AgentEvent {
			loop {
				let event = tokio::time::timeout(Duration::from_secs(10), self.events.recv())
					.await
					.expect("event in time")
					.expect("session alive");
				if !matches!(
					event,
					AgentEvent::AvailableCommands(_)
						| AgentEvent::Account { .. }
						| AgentEvent::Run(specialist::Update::Recovering { .. })
				) {
					return event;
				}
			}
		}

		/// No further event arrives within a short wait.
		async fn quiet(&mut self) {
			if let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(300), self.events.recv()).await {
				panic!("unexpected event: {event:?}");
			}
		}
	}

	fn chat_spec() -> AgentSpec {
		AgentSpec::chat("octoweb:assistant", Some("sess-1".into()), "default", None)
	}

	fn run_frame(seq: u64, event: serde_json::Value) -> serde_json::Value {
		serde_json::json!({"type":"run_event", "session_id":"sess-1", "run_id":"tap-general-abcdef", "role":"developer:general", "seq":seq, "event":event})
	}

	fn running_runs() -> specialist::Cache {
		let mut streams = specialist::Streams::default();
		streams.recover(specialist::Cache::default());
		let mut cache = specialist::Cache::default();
		for (seq, event) in [
			(1, serde_json::json!({"kind":"status","status":"running"})),
			(2, serde_json::json!({"kind":"assistant","content":"saved "})),
		] {
			for update in streams.event(serde_json::from_value(run_frame(seq, event)).unwrap()) {
				cache.apply(&update);
			}
		}
		cache
	}

	#[tokio::test]
	async fn terminal_before_bind_ready_recovers_the_full_saved_transcript() {
		let (tx, mut rx) = mpsc::unbounded_channel();
		let events = Events {
			tx,
			wake: Arc::new(|| {}),
		};
		let (mut session, mut sent) = session_fixture(&events);
		let mut cached = running_runs();
		let frames = [
			serde_json::json!({"type":"ack","request_id":"bind","capabilities":["run_events_v1"]}),
			run_frame(5, serde_json::json!({"kind":"status","status":"done"})),
			serde_json::json!({"type":"status","session_id":"sess-1","message":"ready"}),
		];
		let mut incoming = futures::stream::iter(frames.into_iter().map(|frame| Ok(Message::text(frame.to_string()))));
		session
			.bind(&mut incoming, std::path::Path::new("/tmp"), "octoweb:assistant")
			.await
			.unwrap();
		assert!(rx.try_recv().is_err(), "partial bind transcript escaped");
		assert!(matches!(sent.try_recv().unwrap(), Message::Text(_))); // bind
		assert!(sent.try_recv().is_err());
		assert_eq!(cached.values().next().unwrap().seq, 2);
		session.recover_runs(cached.clone()).await.unwrap();
		assert!(matches!(
			rx.try_recv().unwrap(),
			AgentEvent::Run(specialist::Update::Recovering { .. })
		));
		let Message::Text(request) = sent.try_recv().unwrap() else {
			panic!("run_log")
		};
		let request: serde_json::Value = serde_json::from_str(&request).unwrap();
		assert_eq!(request["type"], "run_log");
		session.on_frame(serde_json::json!({"type":"run_log","session_id":"sess-1","run_id":"tap-general-abcdef","role":"developer:general","status":"done","seq":5,"truncated":false,"events":[{"kind":"assistant","content":"saved and recovered"},{"kind":"status","status":"done"}],"request_id":request["request_id"]})).await.unwrap();
		let AgentEvent::Run(update) = rx.try_recv().unwrap() else {
			panic!("recovered transcript")
		};
		cached.apply(&update);
		let run = cached.values().next().unwrap();
		assert_eq!(run.seq, 5);
		assert_eq!(run.status, specialist::Status::Done);
		assert_eq!(
			run.events[0],
			specialist::Event::Assistant {
				content: "saved and recovered".into()
			}
		);
		assert!(run.complete);
		assert!(rx.try_recv().is_err());
	}

	fn session_fixture(
		events: &Events,
	) -> (
		Session<'_, impl futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin>,
		futures::channel::mpsc::UnboundedReceiver<Message>,
	) {
		let (sent, frames) = futures::channel::mpsc::unbounded::<Message>();
		let sink = sent.sink_map_err(|_| tokio_tungstenite::tungstenite::Error::ConnectionClosed);
		let deadline = Instant::now() + CANCEL_GRACE;
		let session = Session {
			events,
			sink,
			session_id: "sess-1".into(),
			next_id: 0,
			pending: HashMap::from([("pending".into(), Sent::Probe)]),
			current: Some(Sent::UserCommand {
				text: "parent".into(),
				ends_on_cost: true,
			}),
			current_id: Some("parent-id".into()),
			tagged_costs: true,
			run_events: true,
			runs: specialist::Streams::default(),
			queue: VecDeque::new(),
			busy_retries: 2,
			busy_retry_at: Some(deadline),
			busy_since: Some(deadline),
			injected_open: true,
			swallow_cost: true,
			cancel_deadline: Some(deadline),
			last_frame: deadline,
			asking_user: HashSet::from(["ask".into()]),
		};
		(session, frames)
	}

	#[tokio::test]
	async fn run_frames_and_replay_never_change_parent_turn_state() {
		let (tx, mut rx) = mpsc::unbounded_channel();
		let events = Events {
			tx,
			wake: Arc::new(|| {}),
		};
		let (mut session, mut frames) = session_fixture(&events);
		let deadline = session.cancel_deadline.unwrap();
		session.recover_runs(specialist::Cache::default()).await.unwrap();
		let mut cache = specialist::Cache::default();
		let payloads = [
			serde_json::json!({"kind":"prompt","content":"task"}),
			serde_json::json!({"kind":"assistant","content":"child"}),
			serde_json::json!({"kind":"thinking","content":"reason"}),
			serde_json::json!({"kind":"tool_use","tool":"render_ui","tool_id":"child-ask","params":{}}),
			serde_json::json!({"kind":"tool_result","tool_id":"ask","content":"result","success":true}),
			serde_json::json!({"kind":"usage","cost":0.1,"input_tokens":10,"output_tokens":20}),
			serde_json::json!({"kind":"status","status":"failed","error":"child failed"}),
		];
		for (i, event) in payloads.into_iter().enumerate() {
			session
				.on_frame(run_frame(u64::try_from(i).unwrap() + 1, event))
				.await
				.unwrap();
			let AgentEvent::Run(update) = rx.try_recv().unwrap() else {
				panic!("run mapped into parent event")
			};
			cache.apply(&update);
			if i == 6 {
				cache.apply(&specialist::Update::Error {
					run_id: "tap-general-abcdef".into(),
					error: "missing activity".into(),
				});
				session.recover_runs(cache.clone()).await.unwrap();
				assert!(matches!(
					rx.try_recv().unwrap(),
					AgentEvent::Run(specialist::Update::Recovering { .. })
				));
				let Message::Text(request) = frames.try_recv().unwrap() else {
					panic!("run_log request")
				};
				let request: serde_json::Value = serde_json::from_str(&request).unwrap();
				session
					.on_frame(
						serde_json::json!({"type":"ack","message_type":"run_log","request_id":request["request_id"]}),
					)
					.await
					.unwrap();
				session.on_frame(serde_json::json!({"type":"run_log","session_id":"sess-1","run_id":"tap-general-abcdef","status":"unknown","seq":0,"truncated":false,"events":[],"error":"run not found","request_id":request["request_id"]})).await.unwrap();
				assert!(matches!(
					rx.try_recv().unwrap(),
					AgentEvent::Run(specialist::Update::Error { .. })
				));
			}

			assert!(
				matches!(&session.current, Some(Sent::UserCommand { text, ends_on_cost: true }) if text == "parent")
			);
			assert_eq!(session.current_id.as_deref(), Some("parent-id"));
			assert!(matches!(session.pending.get("pending"), Some(Sent::Probe)));
			assert_eq!(session.cancel_deadline, Some(deadline));
			assert_eq!(session.asking_user, HashSet::from(["ask".into()]));
			assert!(session.swallow_cost);
			assert!(session.injected_open);
			assert_eq!(session.busy_retries, 2);
			assert_eq!(session.busy_retry_at, Some(deadline));
			assert_eq!(session.busy_since, Some(deadline));
			assert_eq!(session.last_frame, deadline);
			assert!(session.queue.is_empty());
		}
		// Between turns there is still no parent turn to open or finish.
		session.runs = specialist::Streams::default();
		session.recover_runs(specialist::Cache::default()).await.unwrap();
		session.current = None;
		session.current_id = None;
		session.cancel_deadline = None;
		session.injected_open = false;
		session.swallow_cost = false;
		session.asking_user.clear();
		session
			.on_frame(run_frame(1, serde_json::json!({"kind":"status","status":"running"})))
			.await
			.unwrap();
		assert!(session.current.is_none() && session.current_id.is_none());
		assert!(session.cancel_deadline.is_none() && session.asking_user.is_empty());
		assert!(!session.injected_open && !session.swallow_cost);
		let AgentEvent::Run(update) = rx.try_recv().unwrap() else {
			panic!("expected independent run")
		};
		let mut runs = specialist::Cache::default();
		runs.apply(&update);
		assert!(frames.try_recv().is_err(), "run traffic sent a parent request");
		session.run_events = false;
		session.recover_runs(runs.clone()).await.unwrap();
		assert!(frames.try_recv().is_err(), "recovery sent without capability");
		let error = session
			.on_frame(run_frame(
				9,
				serde_json::json!({"kind":"assistant","content":"unsupported"}),
			))
			.await
			.unwrap_err();
		assert_eq!(error.to_string(), "received run_event without run_events_v1");
		session.run_events = true;
		session.request_run_logs().await.unwrap();
		let Message::Text(text) = frames.try_recv().unwrap() else {
			panic!("expected run_log")
		};
		let frame: serde_json::Value = serde_json::from_str(&text).unwrap();
		assert_eq!(frame["type"], "run_log");
		assert_eq!(frame["run_id"], "tap-general-abcdef");
		assert_eq!(session.pending.len(), 1);
		assert!(session.current.is_none() && session.current_id.is_none());
	}

	#[tokio::test]
	async fn bind_capability_gates_replay_and_streams_runs_between_parent_turns() {
		for supported in [false, true] {
			let endpoint = mock_server(move |mut ws| async move {
				let caps: &[&str] = if supported { &["run_events_v1"] } else { &[] };
				accept_session_with_capabilities(&mut ws, caps).await;
				if supported {
					let replay = recv(&mut ws).await;
					assert_eq!(replay["type"], "run_log");
					ack(&mut ws, &replay).await;
					send(&mut ws, run_frame(2, serde_json::json!({"kind":"assistant","content":"duplicate"}))).await;
					send(&mut ws, run_frame(3, serde_json::json!({"kind":"assistant","content":" live"}))).await;
					send(&mut ws, serde_json::json!({"type":"run_log","session_id":"sess-1","run_id":"tap-general-abcdef","role":"developer:general","status":"running","source":"live","seq":2,"truncated":false,"events":[{"kind":"assistant","content":"snapshot"}],"request_id":replay["request_id"]})).await;
				}
				let message = recv(&mut ws).await;
				assert_eq!(message["type"], "message");
				ack(&mut ws, &message).await;
				send(&mut ws, serde_json::json!({"type":"assistant","content":"parent"})).await;
				send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
				if supported {
					send(&mut ws, run_frame(4, serde_json::json!({"kind":"status","status":"done"}))).await;
				}
				let _ = ws.next().await;
			}).await;
			let mut client = start_with_runs(endpoint, chat_spec(), running_runs());
			assert!(matches!(client.next().await, AgentEvent::Connected(_)));
			client.prompt("parent prompt");
			if supported {
				let AgentEvent::Run(specialist::Update::Snapshot { run }) = client.next().await else {
					panic!("expected run replay")
				};
				assert_eq!(run.seq, 3);
				assert_eq!(
					run.events,
					VecDeque::from([specialist::Event::Assistant {
						content: "snapshot live".into()
					}])
				);
			}
			assert!(matches!(client.next().await, AgentEvent::Chunk(text) if text == "parent"));
			assert!(matches!(client.next().await, AgentEvent::Done));
			if supported {
				assert!(
					matches!(client.next().await, AgentEvent::Run(specialist::Update::Delta { delta }) if delta.status == specialist::Status::Done)
				);
			}
			client.quiet().await;
		}
	}

	#[tokio::test]
	async fn binds_the_session_and_streams_a_turn_to_done() {
		let endpoint = mock_server(|mut ws| async move {
            let bind = accept_session(&mut ws).await;
            assert_eq!(bind["session_id"], "sess-1");
            assert_eq!(bind["role"], "octoweb:assistant");
            assert_eq!(bind["cwd"], "/tmp");

            let msg = recv(&mut ws).await;
            assert_eq!((msg["type"].as_str(), msg["content"].as_str()), (Some("message"), Some("hi")));
            ack(&mut ws, &msg).await;
            send(&mut ws, serde_json::json!({"type": "assistant", "content": "Hel", "session_id": "sess-1"})).await;
            send(&mut ws, serde_json::json!({"type": "tool_use", "tool": "browser_navigate", "tool_id": "t1", "server": "octoweb", "params": {"url": "x"}, "session_id": "sess-1"})).await;
            send(&mut ws, serde_json::json!({"type": "tool_result", "tool": "browser_navigate", "tool_id": "t1", "server": "octoweb", "content": "{\"ok\":true}", "success": true, "session_id": "sess-1"})).await;
            send(&mut ws, serde_json::json!({"type": "assistant", "content": "lo", "session_id": "sess-1"})).await;
            send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
            let _ = ws.next().await;
        })
        .await;
		let mut client = start(endpoint, chat_spec());

		assert!(matches!(client.next().await, AgentEvent::Connected(id) if id == "sess-1"));
		client.prompt("hi");
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "Hel"));
		assert!(matches!(
			client.next().await,
			AgentEvent::ToolStart { id, title, raw_input: Some(_), .. } if id == "t1" && title == "browser_navigate"
		));
		assert!(matches!(
			client.next().await,
			AgentEvent::ToolUpdate { status, raw_output: Some(out), .. } if status == "completed" && out["ok"] == true
		));
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "lo"));
		assert!(matches!(client.next().await, AgentEvent::Done));
		client.quiet().await;
	}

	#[tokio::test]
	async fn cancel_stops_the_turn_and_reports_cancelled() {
		let endpoint = mock_server(|mut ws| async move {
			accept_session(&mut ws).await;
			let msg = recv(&mut ws).await;
			ack(&mut ws, &msg).await;
			send(
				&mut ws,
				serde_json::json!({"type": "assistant", "content": "working", "session_id": "sess-1"}),
			)
			.await;
			let cancel = recv(&mut ws).await;
			assert_eq!(
				(cancel["type"].as_str(), cancel["session_id"].as_str()),
				(Some("cancel"), Some("sess-1"))
			);
			ack(&mut ws, &cancel).await;
			send(&mut ws, cost("sess-1", serde_json::json!({"cancelled": true}))).await;
			let _ = ws.next().await;
		})
		.await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("long task");
		assert!(matches!(client.next().await, AgentEvent::Chunk(_)));
		client.requests.send(Request::Cancel).unwrap();
		assert!(matches!(client.next().await, AgentEvent::Cancelled));
		client.quiet().await;
	}

	#[tokio::test]
	async fn a_prompt_refused_as_busy_runs_after_the_background_turn() {
		let endpoint = mock_server(|mut ws| async move {
            accept_session(&mut ws).await;
            let first = recv(&mut ws).await;
            ack(&mut ws, &first).await;
            send(&mut ws, serde_json::json!({"type": "error", "message": "Session 'sess-1' is busy processing another request. Please wait."})).await;
            // The session's own background turn (a specialist reply) finishes.
            send(&mut ws, serde_json::json!({"type": "injected", "source_kind": "tap_run", "source_label": "agent reviewer", "content": "LGTM", "session_id": "sess-1"})).await;
            send(&mut ws, serde_json::json!({"type": "assistant", "content": "Reviewer agrees.", "session_id": "sess-1"})).await;
            send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
            // The refused prompt comes again, and now runs.
            let again = recv(&mut ws).await;
            assert_eq!(again["content"], "do it");
            assert_ne!(again["request_id"], first["request_id"]);
            ack(&mut ws, &again).await;
            send(&mut ws, serde_json::json!({"type": "assistant", "content": "Done it.", "session_id": "sess-1"})).await;
            send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
            let _ = ws.next().await;
        })
        .await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("do it");
		assert!(matches!(client.next().await, AgentEvent::Injected(t) if t == "[agent reviewer] LGTM"));
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "Reviewer agrees."));
		// The agent's own turn ending is not the waiting prompt's end.
		assert!(matches!(client.next().await, AgentEvent::BackgroundDone));
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "Done it."));
		assert!(matches!(client.next().await, AgentEvent::Done));
		client.quiet().await;
	}

	#[tokio::test]
	async fn cancelling_a_prompt_waiting_on_a_busy_session_drops_it() {
		let endpoint = mock_server(|mut ws| async move {
            accept_session(&mut ws).await;
            let msg = recv(&mut ws).await;
            ack(&mut ws, &msg).await;
            send(&mut ws, serde_json::json!({"type": "error", "message": "Session 'sess-1' is busy processing another request. Please wait."})).await;
            // The background turn ends after the stop; the dropped prompt must
            // not come back.
            tokio::time::sleep(Duration::from_millis(500)).await;
            send(&mut ws, serde_json::json!({"type": "injected", "source_kind": "schedule", "source_label": "s", "content": "x", "session_id": "sess-1"})).await;
            send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
            if let Ok(Some(Ok(Message::Text(text)))) =
                tokio::time::timeout(Duration::from_millis(800), ws.next()).await
            {
                panic!("a cancelled prompt was resent: {text}");
            }
        })
        .await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("queued");
		// Give the busy refusal time to arrive, then stop.
		tokio::time::sleep(Duration::from_millis(100)).await;
		client.requests.send(Request::Cancel).unwrap();
		assert!(matches!(client.next().await, AgentEvent::Cancelled));
	}

	#[tokio::test]
	async fn busy_refusals_never_surface_while_the_session_frees_up() {
		// More refusals than any fixed retry budget, each freed by a background
		// turn's cost: the prompt keeps waiting and finally runs, no error.
		let endpoint = mock_server(|mut ws| async move {
            accept_session(&mut ws).await;
            for round in 0..60 {
                let msg = recv(&mut ws).await;
                assert_eq!(msg["content"], "persist", "round {round}");
                ack(&mut ws, &msg).await;
                send(&mut ws, serde_json::json!({"type": "error", "message": "Session 'sess-1' is busy processing another request. Please wait."})).await;
                send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
            }
            let msg = recv(&mut ws).await;
            ack(&mut ws, &msg).await;
            send(&mut ws, serde_json::json!({"type": "assistant", "content": "finally", "session_id": "sess-1"})).await;
            send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
            let _ = ws.next().await;
        })
        .await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("persist");
		loop {
			match client.next().await {
				// Each freed lock (a background turn's cost) is the agent's
				// own turn ending — never the waiting prompt's Done.
				AgentEvent::BackgroundDone => continue,
				AgentEvent::Chunk(t) => {
					assert_eq!(t, "finally");
					break;
				}
				other => panic!("busy surfaced as {other:?}"),
			}
		}
		assert!(matches!(client.next().await, AgentEvent::Done));
	}

	#[tokio::test]
	async fn dropping_a_handle_mid_turn_cancels_the_turn_on_the_server() {
		let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
		let endpoint = mock_server(|mut ws| async move {
			accept_session(&mut ws).await;
			let msg = recv(&mut ws).await;
			ack(&mut ws, &msg).await;
			send(
				&mut ws,
				serde_json::json!({"type": "assistant", "content": "working", "session_id": "sess-1"}),
			)
			.await;
			let cancel = recv(&mut ws).await;
			assert_eq!(cancel["type"], "cancel");
			let _ = cancelled_tx.send(());
		})
		.await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("long");
		assert!(matches!(client.next().await, AgentEvent::Chunk(_)));
		drop(client.requests);
		tokio::time::timeout(Duration::from_secs(5), cancelled_rx)
			.await
			.expect("cancel sent before disconnecting")
			.unwrap();
	}

	#[tokio::test]
	async fn a_tagged_cost_ends_the_prompt_even_when_a_drained_turn_sent_none() {
		// The stuck-forever case: a specialist reply drained ahead of the prompt
		// failed and closed with no cost of its own. The prompt's cost carries
		// its request id, so it still ends the prompt.
		let endpoint = mock_server(|mut ws| async move {
            send(&mut ws, serde_json::json!({"type": "status", "message": "Connected"})).await;
            let bind = recv(&mut ws).await;
            send(&mut ws, serde_json::json!({
                "type": "ack", "request_id": "bind", "message_type": "session", "session_id": "sess-1",
                "status": "received", "capabilities": ["message_attachments_v1", "session_control_v1"]
            }))
            .await;
            send(&mut ws, serde_json::json!({"type": "status", "message": "Session resumed", "session_id": bind["session_id"]})).await;
            let probe = recv(&mut ws).await;
            ack(&mut ws, &probe).await;
            send(&mut ws, serde_json::json!({"type": "status", "message": "ok", "session_id": "sess-1", "data": {"command_type": "usage"}})).await;

            let msg = recv(&mut ws).await;
            ack(&mut ws, &msg).await;
            send(&mut ws, serde_json::json!({"type": "injected", "source_kind": "tap_run", "source_label": "content:social", "content": "report", "session_id": "sess-1"})).await;
            send(&mut ws, serde_json::json!({"type": "error", "message": "Error processing injected message: provider down"})).await;
            send(&mut ws, serde_json::json!({"type": "assistant", "content": "All good.", "session_id": "sess-1"})).await;
            send(&mut ws, cost("sess-1", serde_json::json!({"request_id": msg["request_id"]}))).await;
            let _ = ws.next().await;
        })
        .await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("So all good?");
		assert!(matches!(client.next().await, AgentEvent::Injected(_)));
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t.contains("provider down")));
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "All good."));
		assert!(matches!(client.next().await, AgentEvent::Done));
		client.quiet().await;
	}

	#[tokio::test]
	async fn a_failed_turn_is_one_turn_error_and_the_session_keeps_working() {
		let endpoint = mock_server(|mut ws| async move {
			accept_session(&mut ws).await;
			let msg = recv(&mut ws).await;
			ack(&mut ws, &msg).await;
			send(
				&mut ws,
				serde_json::json!({"type": "error", "message": "Error: provider down"}),
			)
			.await;
			send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
			let next = recv(&mut ws).await;
			ack(&mut ws, &next).await;
			send(
				&mut ws,
				serde_json::json!({"type": "assistant", "content": "back", "session_id": "sess-1"}),
			)
			.await;
			send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
			let _ = ws.next().await;
		})
		.await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("one");
		assert!(matches!(client.next().await, AgentEvent::TurnError(m) if m == "Error: provider down"));
		// The failed turn's trailing cost closes nothing.
		client.quiet().await;
		client.prompt("two");
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "back"));
		assert!(matches!(client.next().await, AgentEvent::Done));
	}

	#[tokio::test]
	async fn a_slash_prompt_runs_as_a_command_and_shows_its_result() {
		let endpoint = mock_server(|mut ws| async move {
			accept_session(&mut ws).await;
			let cmd = recv(&mut ws).await;
			assert_eq!(cmd["type"], "command");
			assert_eq!(cmd["command"], "model");
			assert_eq!(cmd["args"], serde_json::json!(["openrouter:x"]));
			ack(&mut ws, &cmd).await;
			send(
				&mut ws,
				serde_json::json!({"type": "status", "message": "ok", "session_id": "sess-1", "data": {"command_type": "model"}}),
			)
			.await;
			let _ = ws.next().await;
		})
		.await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		client.prompt("/model openrouter:x");
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "Command executed."));
		assert!(matches!(client.next().await, AgentEvent::Done));
	}

	#[tokio::test]
	async fn a_background_turn_streams_and_ends_with_done() {
		let endpoint = mock_server(|mut ws| async move {
            accept_session(&mut ws).await;
            send(&mut ws, serde_json::json!({"type": "injected", "source_kind": "schedule", "source_label": "schedule ab12", "content": "<system-reminder>x</system-reminder>check mail", "session_id": "sess-1"})).await;
            send(&mut ws, serde_json::json!({"type": "assistant", "content": "Checked.", "session_id": "sess-1"})).await;
            send(&mut ws, cost("sess-1", serde_json::json!({}))).await;
            let _ = ws.next().await;
        })
        .await;
		let mut client = start(endpoint, chat_spec());
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		assert!(matches!(client.next().await, AgentEvent::Injected(t) if t == "[schedule ab12] check mail"));
		assert!(matches!(client.next().await, AgentEvent::Chunk(t) if t == "Checked."));
		assert!(matches!(client.next().await, AgentEvent::BackgroundDone));
	}

	#[tokio::test]
	async fn a_one_shot_session_is_closed_on_the_server_when_dropped() {
		let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
		let endpoint = mock_server(|mut ws| async move {
			let bind = accept_session(&mut ws).await;
			let sid = bind["session_id"].as_str().unwrap().to_string();
			assert!(sid.starts_with("octoweb-"), "{sid}");
			let close = recv(&mut ws).await;
			assert_eq!(close["type"], "close");
			assert_eq!(close["session_id"].as_str(), Some(sid.as_str()));
			send(
				&mut ws,
				serde_json::json!({"type": "status", "message": "closed", "session_id": sid, "data": {"command_type": "close"}}),
			)
			.await;
			let _ = closed_tx.send(());
		})
		.await;
		let mut client = start(endpoint, AgentSpec::one_shot("octoweb:mail-reader", "default", None));
		assert!(matches!(client.next().await, AgentEvent::Connected(_)));
		drop(client.requests);
		tokio::time::timeout(Duration::from_secs(5), closed_rx)
			.await
			.expect("close sent in time")
			.unwrap();
		assert!(client.done.await.unwrap().is_ok());
	}

	#[tokio::test]
	async fn a_bind_the_server_refuses_ends_the_session_with_its_reason() {
		let endpoint = mock_server(|mut ws| async move {
			send(&mut ws, serde_json::json!({"type": "status", "message": "Connected"})).await;
			let bind = recv(&mut ws).await;
			ack(&mut ws, &bind).await;
			send(
				&mut ws,
				serde_json::json!({"type": "error", "message": "Failed to resolve role 'x:y': no such tap"}),
			)
			.await;
			let _ = ws.next().await;
		})
		.await;
		let client = start(endpoint, AgentSpec::chat("x:y", None, "default", None));
		let error = client.done.await.unwrap().unwrap_err().to_string();
		assert!(error.contains("Failed to resolve role 'x:y'"), "{error}");
	}

	#[test]
	fn typed_command_results_read_as_text() {
		assert_eq!(
			render_command_output(&serde_json::json!({"command_type": "model"})),
			"Command executed."
		);
		assert_eq!(
			render_command_output(&serde_json::json!({"command_type": "done", "message": "Nothing to compress"})),
			"Nothing to compress"
		);
		let rich = render_command_output(&serde_json::json!({"command_type": "info", "tokens": 5, "cost": 1}));
		assert!(rich.contains("\"tokens\": 5"), "{rich}");
	}
}
