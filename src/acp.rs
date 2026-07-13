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
    AvailableCommandInput, CancelNotification, ContentBlock, ContentChunk, ImageContent,
    Implementation, InitializeRequest, NewSessionRequest, PermissionOptionId, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};

/// Events streamed from the agent back to the main thread.
#[derive(Debug)]
pub enum AgentEvent {
    /// ACP session is ready — agent connected and initialized.
    /// Carries the ACP protocol session id so the main thread can pass it
    /// back as `--resume <id>` when force-respawning the subprocess.
    Connected(String),
    /// A text chunk from the agent's response (streaming).
    Chunk(String),
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
}

impl AcpHandle {
    /// Spawn the agent process described by `cmd` (e.g. `"octomind acp"` or
    /// `"octomind acp doctor:blood"`), initialize the ACP session, return the handle.
    /// Initialization is async — watch `rx` for `AgentEvent::Connected` or `AgentEvent::Error`.
    ///
    /// `wake` is called from the ACP thread whenever an event is pushed — use it to
    /// poke the main event loop out of `ControlFlow::Wait`.
    pub fn connect(cmd: &str, wake: impl Fn() + Send + Sync + 'static) -> anyhow::Result<Self> {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
        let (prompt_tx, prompt_rx) = tokio::sync::mpsc::unbounded_channel::<PromptMessage>();
        let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
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
                    prompt_rx,
                    cancel_rx,
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
}

async fn init_session(
    tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    mut prompt_rx: tokio::sync::mpsc::UnboundedReceiver<PromptMessage>,
    mut cancel_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    program: String,
    args: Vec<String>,
) -> anyhow::Result<()> {
    // Run the agent inside octoweb's internal workspace dir (not the user's
    // home). Combined with `--sandbox` (injected in `connect`), this confines
    // all of the agent's filesystem writes to this dir.
    let workspace = crate::a2ui_render_ui::workspace_dir();
    let _ = std::fs::create_dir_all(&workspace);

    let mut child = tokio::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .current_dir(&workspace)
        .kill_on_drop(true)
        .spawn()?;

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

                // Process prompts sequentially — one at a time.
                while let Some(msg) = prompt_rx.recv().await {
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
                                        Ok(_) => { let _ = tx.send(AgentEvent::Done); }
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

                Ok(())
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("ACP connection error: {e}"))?;

    Ok(())
}
