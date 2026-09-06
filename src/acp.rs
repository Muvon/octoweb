/// ACP (Agent Client Protocol) integration.
///
/// Spawns `octomind acp` as a subprocess and communicates over stdio using
/// the agent-client-protocol crate. Streams agent response chunks back to
/// the main thread via a tokio mpsc channel.
///
/// Flow:
///   1. `AcpHandle::connect()` spawns the agent process and initializes the session.
///   2. `AcpHandle::send_prompt(text)` sends a prompt; the agent streams chunks via
///      `session_notification` callbacks, and `conn.prompt()` returns when done.
///   3. The main thread polls `AcpHandle::rx` for `AgentEvent` variants.
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use agent_client_protocol::schema::v1::{
    AvailableCommandInput, CancelNotification, ClientRequest, ContentBlock, ContentChunk,
    ExtRequest, ImageContent, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOptionId, PermissionOptionKind, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionId,
    SessionNotification, SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use serde_json::value::RawValue;

/// Events streamed from the agent back to the main thread.
#[derive(Debug)]
pub enum AgentEvent {
    /// ACP session is ready — agent connected and initialized.
    /// Carries the ACP protocol session id so the main thread can pass it
    /// back as `--resume <id>` when force-respawning the subprocess.
    Connected(String),
    /// A text chunk from the agent's response (streaming).
    Chunk(String),
    /// A message injected into the session by the agent runtime — a specialist
    /// (tap-run) reply, schedule, webhook, etc. Arrives as a `UserMessageChunk`
    /// with a `[<source label>] ` prefix. Rendered as its own bubble, never
    /// merged into the streamed agent response.
    Injected(String),
    /// An image from the agent's response.
    Image { data: String, mime_type: String },
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
    /// Agent finished responding (conn.prompt() returned).
    Done,
    /// Agent was cancelled by user.
    Cancelled,
    /// Agent or connection error.
    Error(String),
    /// Agent sent updated list of available slash commands.
    AvailableCommands(Vec<CommandInfo>),
    /// Octomind account status, parsed from a `/usage` ext call. Drives the
    /// sidebar's login chip and signed-out / over-quota banner.
    Account {
        signed_in: bool,
        /// "email (plan)" when known.
        account: Option<String>,
        /// A spend window is committed at or over its cap.
        over_quota: bool,
        /// Short human summary for the banner, e.g. "$3.40 / $5.00 (week)".
        summary: Option<String>,
    },
    /// A device-login flow started (from `/login`). The client opens `url` in a
    /// browser tab, shows `code`, and polls `/usage` until signed in.
    LoginStarted {
        url: String,
        code: String,
        already_signed_in: bool,
    },
    /// Emitted once, right after spawn — the OS pid of the agent subprocess.
    /// Used by the A2UI watcher to route surface envelopes back to the
    /// session whose octomind invoked `render_ui` (the bash script stamps
    /// its parent pid into the envelope file).
    ProcessPid(u32),
}

/// A prompt with optional image attachments.
pub struct PromptMessage {
    pub text: String,
    pub images: Vec<(String, String)>, // (base64_data, mime_type)
}

/// Simplified representation of an ACP available command for the UI.
#[derive(Debug, Clone)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    /// Optional input hint (e.g. "describe what to search for").
    pub hint: Option<String>,
}

/// Forward one `session/update` to the main thread as `AgentEvent`s.
/// Registered as the notification handler on the client connection —
/// fs/terminal requests stay unregistered so the SDK answers them with
/// method_not_found (not needed for chat).
fn handle_session_update(
    tx: &tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    wake: &std::sync::Arc<dyn Fn() + Send + Sync>,
    update: SessionUpdate,
) {
    match update {
        SessionUpdate::AgentMessageChunk(ContentChunk { content, .. }) => match content {
            ContentBlock::Text(t) if !t.text.is_empty() => {
                let _ = tx.send(AgentEvent::Chunk(t.text));
                wake();
            }
            ContentBlock::Image(img) => {
                let _ = tx.send(AgentEvent::Image {
                    data: img.data,
                    mime_type: img.mime_type,
                });
                wake();
            }
            ContentBlock::ResourceLink(r) if !r.uri.is_empty() => {
                let _ = tx.send(AgentEvent::Chunk(r.uri));
                wake();
            }
            _ => {}
        },
        // Inbox injections (specialist replies, schedules, webhooks) — the
        // only user-side chunks octomind emits mid-session.
        SessionUpdate::UserMessageChunk(ContentChunk {
            content: ContentBlock::Text(t),
            ..
        }) if !t.text.is_empty() => {
            let _ = tx.send(AgentEvent::Injected(t.text));
            wake();
        }
        SessionUpdate::ToolCall(tc) => {
            let locations: Vec<String> = tc
                .locations
                .iter()
                .map(|l| l.path.to_string_lossy().to_string())
                .collect();
            let _ = tx.send(AgentEvent::ToolStart {
                id: tc.tool_call_id.0.to_string(),
                title: tc.title,
                kind: format!("{:?}", tc.kind).to_lowercase(),
                raw_input: tc.raw_input.clone(),
                locations,
            });
            wake();
        }
        SessionUpdate::ToolCallUpdate(upd) => {
            let status = match upd.fields.status {
                Some(s) => format!("{:?}", s).to_lowercase(),
                None => String::new(),
            };
            let _ = tx.send(AgentEvent::ToolUpdate {
                id: upd.tool_call_id.0.to_string(),
                title: upd.fields.title.clone(),
                status,
                raw_output: upd.fields.raw_output.clone(),
            });
            wake();
        }
        SessionUpdate::AvailableCommandsUpdate(update) => {
            let commands: Vec<CommandInfo> = update
                .available_commands
                .into_iter()
                .map(|cmd| {
                    let hint = cmd.input.and_then(|inp| match inp {
                        AvailableCommandInput::Unstructured(u) => Some(u.hint),
                        _ => None,
                    });
                    CommandInfo {
                        name: cmd.name,
                        description: cmd.description,
                        hint,
                    }
                })
                .collect();
            let _ = tx.send(AgentEvent::AvailableCommands(commands));
            wake();
        }
        _ => {}
    }
}

/// Handle passed to the main thread. Send prompts, receive events via `rx`.
pub struct AcpHandle {
    /// Receive `AgentEvent`s from the background ACP thread.
    pub rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    /// Send prompts to the background ACP thread.
    prompt_tx: tokio::sync::mpsc::UnboundedSender<PromptMessage>,
    /// Signal the ACP thread to cancel the current prompt.
    cancel_tx: tokio::sync::mpsc::UnboundedSender<()>,
    /// Send slash commands (e.g. `/usage`, `/login`) to run as ACP ext calls.
    command_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl AcpHandle {
    /// Spawn the agent process described by `cmd` (e.g. `"octomind acp"` or
    /// `"octomind acp doctor:blood"`), initialize the ACP session, return the handle.
    /// Initialization is async — watch `rx` for `AgentEvent::Connected` or `AgentEvent::Error`.
    ///
    /// `wake` is called from the ACP thread whenever an event is pushed — use it to
    /// poke the main event loop out of `ControlFlow::Wait`.
    /// `mcp_token` identifies the caller to octoweb's MCP server. A chat
    /// session's token names both its workspace and the session itself, so
    /// browser tools act on that workspace's tabs and `render_ui` draws into
    /// that chat, regardless of what the user is looking at. Background agents
    /// (learning, inline edit) get a workspace-only token. `None` means tool
    /// calls land on the default workspace, matching how it behaved before
    /// workspaces existed.
    pub fn connect(
        cmd: &str,
        mcp_token: Option<String>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
        let (prompt_tx, prompt_rx) = tokio::sync::mpsc::unbounded_channel::<PromptMessage>();
        let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(wake);

        // Parse "program arg1 arg2 ..." — simple whitespace split, no shell quoting needed.
        let mut parts = cmd.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty agent command"))?
            .to_string();
        let mut args: Vec<String> = parts.map(str::to_string).collect();
        // Sandbox every agent to its workspace cwd (set in init_session) so its
        // filesystem writes can't escape octoweb's internal dir. Injected here,
        // centrally, instead of at each `octomind acp …` call site.
        args.push("--sandbox".to_string());

        // Spawn a dedicated OS thread with a current_thread runtime + LocalSet.
        // ClientSideConnection is !Send, so it must live entirely on one thread.
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                match init_session(
                    event_tx.clone(),
                    std::sync::Arc::clone(&wake),
                    SessionInbox {
                        prompt_rx,
                        cancel_rx,
                        command_rx,
                    },
                    mcp_token,
                    program,
                    args,
                )
                .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        let _ = event_tx.send(AgentEvent::Error(e.to_string()));
                        wake();
                    }
                }
            });
        });

        Ok(Self {
            rx: event_rx,
            prompt_tx,
            cancel_tx,
            command_tx,
        })
    }

    /// Drain all pending events from the channel without blocking.
    /// Call this on every event loop tick to forward agent events to the UI.
    pub fn poll(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            events.push(ev);
        }
        events
    }

    /// Send a user prompt (with optional images) to the agent.
    /// Returns false if the channel is dead (ACP thread exited).
    pub fn send_prompt(&self, text: String, images: Vec<(String, String)>) -> bool {
        self.prompt_tx.send(PromptMessage { text, images }).is_ok()
    }

    /// Cancel the current prompt. The agent will stop and `AgentEvent::Cancelled` is sent.
    pub fn cancel(&self) {
        let _ = self.cancel_tx.send(());
    }

    /// Run a slash command (e.g. `/usage`, `/login`) as an ACP ext call. The
    /// result comes back as an `AgentEvent` (`Account` or `LoginStarted`).
    /// Returns false if the ACP thread has exited.
    pub fn send_command(&self, command: String) -> bool {
        self.command_tx.send(command).is_ok()
    }
}

/// Run one `/…` slash command over the `_octomind/command` ACP ext method and
/// forward its structured result to the main thread as an `AgentEvent`. Failures
/// are logged, not surfaced — these are background probes, not user actions.
async fn run_ext_command(
    cx: &ConnectionTo<Agent>,
    session_id: &SessionId,
    tx: &tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    wake: &std::sync::Arc<dyn Fn() + Send + Sync>,
    command: &str,
) {
    let params = serde_json::json!({
        "session_id": session_id.0.to_string(),
        "command": command,
        "args": [],
    });
    let Ok(raw) = serde_json::to_string(&params).and_then(RawValue::from_string) else {
        return;
    };
    // Ext method names travel on the wire with a leading `_`; the agent strips it
    // and routes to its `octomind/command` handler.
    let req = ClientRequest::ExtMethodRequest(ExtRequest::new(
        "_octomind/command",
        std::sync::Arc::from(raw),
    ));
    let val = match cx.send_request(req).block_task().await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(command, error = %e, "ACP ext command failed");
            return;
        }
    };

    // Response is octomind's CommandResponse: { success, output, error }.
    let Some(out) = val.get("output") else { return };
    match out.get("command_type").and_then(|v| v.as_str()) {
        Some("usage") => {
            let signed_in = out
                .get("signed_in")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let account = out
                .get("account")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let mut over_quota = false;
            let mut tightest: Option<(f64, String)> = None;
            if let Some(windows) = out.get("windows").and_then(|v| v.as_array()) {
                for w in windows {
                    let f = |k: &str| w.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let cap = f("cap_usd");
                    if cap <= 0.0 {
                        continue;
                    }
                    // Reserved is future burn already committed by cloud machines —
                    // count it against the cap so headroom reads honestly.
                    let committed = f("spent_usd") + f("reserved_usd");
                    if committed >= cap {
                        over_quota = true;
                    }
                    let label = w.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    let frac = committed / cap;
                    if tightest.as_ref().is_none_or(|(best, _)| frac > *best) {
                        tightest = Some((frac, format!("${committed:.2} / ${cap:.2} ({label})")));
                    }
                }
            }
            let _ = tx.send(AgentEvent::Account {
                signed_in,
                account,
                over_quota,
                summary: tightest.map(|(_, t)| t),
            });
            wake();
        }
        Some("login") => {
            let already_signed_in = out
                .get("already_signed_in")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let url = out
                .get("verification_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let code = out
                .get("user_code")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let _ = tx.send(AgentEvent::LoginStarted {
                url,
                code,
                already_signed_in,
            });
            wake();
        }
        _ => {}
    }
}

/// Receiving ends of the `AcpHandle` channels, owned by the ACP thread.
struct SessionInbox {
    prompt_rx: tokio::sync::mpsc::UnboundedReceiver<PromptMessage>,
    cancel_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    command_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
}

async fn init_session(
    tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    inbox: SessionInbox,
    mcp_token: Option<String>,
    program: String,
    args: Vec<String>,
) -> anyhow::Result<()> {
    let SessionInbox {
        mut prompt_rx,
        mut cancel_rx,
        mut command_rx,
    } = inbox;
    // Run the agent inside octoweb's internal workspace dir (not the user's
    // home). Combined with `--sandbox` (injected in `connect`), this confines
    // all of the agent's filesystem writes to this dir.
    let workspace = crate::agent_workspace::workspace_dir();
    let _ = std::fs::create_dir_all(&workspace);

    let mut child = tokio::process::Command::new(&program);
    // The agent's capability manifest forwards this as the
    // `X-Octoweb-Workspace` header, which is how octoweb's MCP server knows
    // which workspace — and which chat session — the call belongs to.
    // Ephemeral, minted per run.
    if let Some(token) = mcp_token {
        child.env("OCTOWEB_MCP_TOKEN", token);
    }
    // The capability manifest templates this into the MCP server URL. Always
    // set it, including on the default port: an agent that resolved a hardcoded
    // 3434 would drive whichever instance owns that port, not the one that
    // spawned it.
    child.env("OCTOWEB_MCP_PORT", crate::mcp::port().to_string());
    let mut child = child
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .current_dir(&workspace)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "The AI agent ({program}) isn't installed.\n\
                     Install it with:\n\
                     curl -fsSL https://raw.githubusercontent.com/muvon/octomind/master/install.sh | bash\n\
                     then set an API key (e.g. OPENROUTER_API_KEY) and reopen this sidebar."
                )
            } else {
                anyhow::anyhow!("failed to start {program}: {e}")
            }
        })?;

    // Surface the spawned pid so main can map A2UI envelopes (which carry
    // the agent's pid via the bash script's $PPID) back to this session.
    if let Some(pid) = child.id() {
        let _ = tx.send(AgentEvent::ProcessPid(pid));
        wake();
    }

    let outgoing = child.stdin.take().unwrap().compat_write();
    let incoming = child.stdout.take().unwrap().compat();

    // Monitor subprocess — if it exits unexpectedly, notify the main thread.
    let tx_exit = tx.clone();
    let wake_exit = std::sync::Arc::clone(&wake);
    tokio::task::spawn_local(async move {
        let status = child.wait().await;
        let msg = match status {
            Ok(s) if s.success() => "agent process exited".to_string(),
            Ok(s) => format!("agent process exited with {s}"),
            Err(e) => format!("agent process error: {e}"),
        };
        let _ = tx_exit.send(AgentEvent::Error(msg));
        wake_exit();
    });

    let activity = std::sync::Arc::new(tokio::sync::Notify::new());

    let notif_tx = tx.clone();
    let notif_wake = std::sync::Arc::clone(&wake);
    let notif_activity = std::sync::Arc::clone(&activity);
    let perm_activity = std::sync::Arc::clone(&activity);

    Client
        .builder()
        .name("octoweb")
        .on_receive_notification(
            async move |args: SessionNotification, _cx| {
                // Reset idle timeout — agent is actively working.
                notif_activity.notify_one();
                handle_session_update(&notif_tx, &notif_wake, args.update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |args: RequestPermissionRequest, responder, _cx| {
                // Reset idle timeout — permission request is also a sign of life.
                perm_activity.notify_one();
                // Auto-approve by selecting the first allow option from the request.
                // Agent is a trusted local process — no need to prompt the user.
                let option_id = args
                    .options
                    .iter()
                    .find(|o| {
                        matches!(
                            o.kind,
                            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                        )
                    })
                    .or_else(|| args.options.first())
                    .map(|o| o.option_id.clone())
                    .unwrap_or_else(|| PermissionOptionId::new("allow"));
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            ByteStreams::new(outgoing, incoming),
            async move |cx: ConnectionTo<Agent>| {
                cx.send_request(
                    InitializeRequest::new(ProtocolVersion::V1).client_info(
                        Implementation::new("octoweb", env!("CARGO_PKG_VERSION"))
                            .title("Octoweb Browser"),
                    ),
                )
                .block_task()
                .await?;

                // Workspace cwd for the session — octomind uses this to discover local
                // tools at `<cwd>/.agents/tools/*` and as the `--sandbox` root. Must match
                // the process cwd set above so tool discovery and the sandbox agree, and so
                // it survives launchd launches where `current_dir()` would be `/`.
                let resp = cx
                    .send_request(NewSessionRequest::new(workspace.clone()))
                    .block_task()
                    .await?;

                let session_id = resp.session_id;
                let _ = tx.send(AgentEvent::Connected(session_id.0.to_string()));
                wake();

                // Keep account/quota status in sync with the server. Probe once
                // immediately so the panel shows login state quickly, then refresh
                // periodically while the session is live so reserved spend and window
                // resets are reflected in the sidebar chip.
                let usage_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                {
                    let cx = cx.clone();
                    let tx = tx.clone();
                    let wake = std::sync::Arc::clone(&wake);
                    let session_id = session_id.clone();
                    let stop = std::sync::Arc::clone(&usage_stop);
                    tokio::task::spawn_local(async move {
                        run_ext_command(&cx, &session_id, &tx, &wake, "/usage").await;
                        const REFRESH_INTERVAL: std::time::Duration =
                            std::time::Duration::from_secs(60);
                        loop {
                            tokio::time::sleep(REFRESH_INTERVAL).await;
                            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                                return;
                            }
                            run_ext_command(&cx, &session_id, &tx, &wake, "/usage").await;
                        }
                    });
                }

                // Serve prompts one at a time, interleaving `/…` ext commands
                // (login, usage refresh) whenever no prompt is in flight.
                loop {
                    let msg = tokio::select! {
                        biased;
                        cmd = command_rx.recv() => match cmd {
                            Some(command) => {
                                run_ext_command(&cx, &session_id, &tx, &wake, &command).await;
                                continue;
                            }
                            None => break, // handle dropped
                        },
                        prompt = prompt_rx.recv() => match prompt {
                            Some(m) => m,
                            None => break, // handle dropped
                        },
                    };

                    let mut content: Vec<ContentBlock> = Vec::new();
                    for (data, mime) in msg.images {
                        content.push(ContentBlock::Image(ImageContent::new(data, mime)));
                    }
                    content.push(ContentBlock::Text(TextContent::new(msg.text)));
                    let prompt_fut = cx
                        .send_request(PromptRequest::new(session_id.clone(), content))
                        .block_task();
                    tokio::pin!(prompt_fut);

                    // Idle timeout: fires only when the agent sends no activity for 20 minutes.
                    // Resets on every session_notification (chunks, tool starts, tool updates)
                    // and on permission requests. The previous 5-minute window produced false
                    // positives on long-running tool calls (large MCP fetches, slow agent steps)
                    // that legitimately produce no intermediate output.
                    const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1200);
                    let idle_timeout = async {
                        loop {
                            tokio::select! {
                                biased; // Check notification first to avoid false timeouts
                                _ = activity.notified() => continue, // Activity received — restart timer
                                _ = tokio::time::sleep(IDLE_TIMEOUT) => break, // No activity for 20 min
                            }
                        }
                    };
                    tokio::pin!(idle_timeout);

                    // After cancel we keep awaiting prompt_fut so the ACP connection stays
                    // clean (response is properly consumed). A 10s deadline prevents hanging
                    // if the agent ignores the cancel notification.
                    let mut cancelled = false;
                    let cancel_deadline = tokio::time::sleep(std::time::Duration::from_secs(86400));
                    tokio::pin!(cancel_deadline);

                    loop {
                        tokio::select! {
                            res = &mut prompt_fut => {
                                if cancelled {
                                    let _ = tx.send(AgentEvent::Cancelled);
                                } else {
                                    match res {
                                        Ok(resp) => {
                                            // A turn that hit a token cap or was refused
                                            // otherwise renders identically to one that
                                            // finished — the reply just stops mid-thought
                                            // and the user re-asks. Say what happened.
                                            let note = match resp.stop_reason {
                                                StopReason::MaxTokens => Some(
                                                    "\n\n_Stopped: the agent hit its token limit — \
                                                     the answer above is incomplete._",
                                                ),
                                                StopReason::MaxTurnRequests => Some(
                                                    "\n\n_Stopped: the agent hit its limit on tool \
                                                     calls for one turn — ask it to continue._",
                                                ),
                                                StopReason::Refusal => Some(
                                                    "\n\n_The agent declined to continue this turn._",
                                                ),
                                                _ => None,
                                            };
                                            if let Some(note) = note {
                                                let _ = tx.send(AgentEvent::Chunk(note.to_string()));
                                            }
                                            let _ = tx.send(AgentEvent::Done);
                                        }
                                        Err(e) => { let _ = tx.send(AgentEvent::Error(e.to_string())); }
                                    }
                                }
                                wake();
                                break;
                            }
                            _ = cancel_rx.recv(), if !cancelled => {
                                // Arm the 10s deadline FIRST, then fire the cancel
                                // notification (a non-blocking channel send in this SDK)
                                // so the loop unwedges even if the agent ignores it.
                                cancelled = true;
                                cancel_deadline.as_mut().reset(tokio::time::Instant::now() + std::time::Duration::from_secs(10));
                                let _ = cx.send_notification(CancelNotification::new(session_id.clone()));
                                // Don't break — keep looping so prompt_fut completes cleanly
                            }
                            _ = &mut cancel_deadline, if cancelled => {
                                // Agent didn't finish within 10s after cancel — give up
                                let _ = tx.send(AgentEvent::Cancelled);
                                wake();
                                break;
                            }
                            _ = &mut idle_timeout, if !cancelled => {
                                // Agent went silent for 20 minutes — considered stuck
                                let _ = tx.send(AgentEvent::Error(
                                    "agent idle for 20 minutes — no response".into(),
                                ));
                                wake();
                                break;
                            }
                        }
                    }
                }

                // Stop the periodic `/usage` refresh task when the session ends.
                usage_stop.store(true, std::sync::atomic::Ordering::Relaxed);

                Ok(())
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("ACP connection error: {e}"))?;

    Ok(())
}
