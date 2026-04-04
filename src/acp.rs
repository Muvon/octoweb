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

use agent_client_protocol::{self as acp, Agent};

/// Events streamed from the agent back to the main thread.
#[derive(Debug)]
pub enum AgentEvent {
    /// ACP session is ready — agent connected and initialized.
    Connected,
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
}

/// A prompt with optional image attachments.
pub struct PromptMessage {
    pub text: String,
    pub images: Vec<(String, String)>, // (base64_data, mime_type)
}

/// Minimal Client impl — handles streaming text chunks and auto-approves permissions.
/// All fs/terminal methods return method_not_found (not needed for chat).
struct BrowserClient {
    tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    /// Notified on every session_notification — resets the idle timeout.
    activity: std::sync::Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait(?Send)]
impl acp::Client for BrowserClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        // Auto-approve by selecting the first allow option from the request.
        // Agent is a trusted local process — no need to prompt the user.
        let option_id = args
            .options
            .iter()
            .find(|o| {
                matches!(
                    o.kind,
                    acp::PermissionOptionKind::AllowOnce | acp::PermissionOptionKind::AllowAlways
                )
            })
            .or_else(|| args.options.first())
            .map(|o| o.option_id.clone())
            .unwrap_or_else(|| acp::PermissionOptionId::new("allow"));
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(option_id)),
        ))
    }

    async fn write_text_file(
        &self,
        _args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal(
        &self,
        _args: acp::KillTerminalRequest,
    ) -> acp::Result<acp::KillTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        // Reset idle timeout — agent is actively working.
        self.activity.notify_one();

        match args.update {
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk { content, .. }) => {
                match content {
                    acp::ContentBlock::Text(t) => {
                        if !t.text.is_empty() {
                            let _ = self.tx.send(AgentEvent::Chunk(t.text));
                            (self.wake)();
                        }
                    }
                    acp::ContentBlock::Image(img) => {
                        let _ = self.tx.send(AgentEvent::Image {
                            data: img.data,
                            mime_type: img.mime_type,
                        });
                        (self.wake)();
                    }
                    acp::ContentBlock::ResourceLink(r) => {
                        if !r.uri.is_empty() {
                            let _ = self.tx.send(AgentEvent::Chunk(r.uri));
                            (self.wake)();
                        }
                    }
                    _ => {}
                }
            }
            acp::SessionUpdate::ToolCall(tc) => {
                let locations: Vec<String> = tc
                    .locations
                    .iter()
                    .map(|l| l.path.to_string_lossy().to_string())
                    .collect();
                let _ = self.tx.send(AgentEvent::ToolStart {
                    id: tc.tool_call_id.0.to_string(),
                    title: tc.title,
                    kind: format!("{:?}", tc.kind).to_lowercase(),
                    raw_input: tc.raw_input.clone(),
                    locations,
                });
                (self.wake)();
            }
            acp::SessionUpdate::ToolCallUpdate(upd) => {
                let status = match upd.fields.status {
                    Some(s) => format!("{:?}", s).to_lowercase(),
                    None => String::new(),
                };
                let _ = self.tx.send(AgentEvent::ToolUpdate {
                    id: upd.tool_call_id.0.to_string(),
                    title: upd.fields.title.clone(),
                    status,
                    raw_output: upd.fields.raw_output.clone(),
                });
                (self.wake)();
            }
            _ => {}
        }
        Ok(())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
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
        let args: Vec<String> = parts.map(str::to_string).collect();

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
    let mut child = tokio::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .current_dir(dirs::home_dir().unwrap_or_else(|| "/".into()))
        .kill_on_drop(true)
        .spawn()?;

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
    let (conn, handle_io) = acp::ClientSideConnection::new(
        BrowserClient {
            tx: tx.clone(),
            wake: std::sync::Arc::clone(&wake),
            activity: std::sync::Arc::clone(&activity),
        },
        outgoing,
        incoming,
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );

    tokio::task::spawn_local(handle_io);

    conn.initialize(
        acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_info(
            acp::Implementation::new("octoweb", env!("CARGO_PKG_VERSION")).title("Octoweb Browser"),
        ),
    )
    .await?;

    let resp = conn
        .new_session(acp::NewSessionRequest::new(
            std::env::current_dir()
                .or_else(|_| dirs::home_dir().ok_or(std::io::Error::other("no home")))
                .unwrap_or_else(|_| "/".into()),
        ))
        .await?;

    let session_id = resp.session_id;
    let _ = tx.send(AgentEvent::Connected);
    wake();

    // Process prompts sequentially — one at a time.
    while let Some(msg) = prompt_rx.recv().await {
        let mut content: Vec<acp::ContentBlock> = Vec::new();
        for (data, mime) in msg.images {
            content.push(acp::ContentBlock::Image(acp::ImageContent::new(data, mime)));
        }
        content.push(msg.text.into());
        let prompt_fut = conn.prompt(acp::PromptRequest::new(session_id.clone(), content));
        tokio::pin!(prompt_fut);

        // Idle timeout: fires only when the agent sends no activity for 5 minutes.
        // Resets on every session_notification (chunks, tool starts, tool updates).
        const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
        let idle_timeout = async {
            loop {
                tokio::select! {
                    biased; // Check notification first to avoid false timeouts
                    _ = activity.notified() => continue, // Activity received — restart timer
                    _ = tokio::time::sleep(IDLE_TIMEOUT) => break, // No activity for 5 min
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
                            Err(_) if cancelled => { let _ = tx.send(AgentEvent::Cancelled); }
                            Err(e) => { let _ = tx.send(AgentEvent::Error(e.to_string())); }
                        }
                    }
                    wake();
                    break;
                }
                _ = cancel_rx.recv(), if !cancelled => {
                    // Cancel received — notify agent, then keep waiting for prompt_fut
                    let _ = conn
                        .cancel(acp::CancelNotification::new(session_id.clone()))
                        .await;
                    cancelled = true;
                    cancel_deadline.as_mut().reset(tokio::time::Instant::now() + std::time::Duration::from_secs(10));
                    // Don't break — keep looping so prompt_fut completes cleanly
                }
                _ = &mut cancel_deadline, if cancelled => {
                    // Agent didn't finish within 10s after cancel — give up
                    let _ = tx.send(AgentEvent::Cancelled);
                    wake();
                    break;
                }
                _ = &mut idle_timeout, if !cancelled => {
                    // Agent went silent for 5 minutes — considered stuck
                    let _ = tx.send(AgentEvent::Error(
                        "agent idle for 5 minutes — no response".into(),
                    ));
                    wake();
                    break;
                }
            }
        }
    }

    Ok(())
}
