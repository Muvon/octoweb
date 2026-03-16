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
    /// A new tool call started (id, title, kind).
    ToolStart {
        id: String,
        title: String,
        kind: String,
    },
    /// An existing tool call was updated (id, optional new title, status).
    ToolUpdate {
        id: String,
        title: Option<String>,
        status: String,
    },
    /// Agent finished responding (conn.prompt() returned).
    Done,
    /// Agent was cancelled by user.
    Cancelled,
    /// Agent or connection error.
    Error(String),
}

/// Minimal Client impl — handles streaming text chunks and auto-approves permissions.
/// All fs/terminal methods return method_not_found (not needed for chat).
struct BrowserClient {
    tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
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
        match args.update {
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk { content, .. }) => {
                let text = match content {
                    acp::ContentBlock::Text(t) => t.text,
                    acp::ContentBlock::ResourceLink(r) => r.uri,
                    _ => return Ok(()),
                };
                if !text.is_empty() {
                    let _ = self.tx.send(AgentEvent::Chunk(text));
                    (self.wake)();
                }
            }
            acp::SessionUpdate::ToolCall(tc) => {
                let _ = self.tx.send(AgentEvent::ToolStart {
                    id: tc.tool_call_id.0.to_string(),
                    title: tc.title,
                    kind: format!("{:?}", tc.kind).to_lowercase(),
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
                    title: upd.fields.title,
                    status,
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
    prompt_tx: tokio::sync::mpsc::UnboundedSender<String>,
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
        let (prompt_tx, prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
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

    /// Send a user prompt to the agent. Non-blocking — chunks arrive via `rx`.
    /// Sends `AgentEvent::Done` when the agent finishes, or `AgentEvent::Error` on failure.
    pub fn send_prompt(&self, text: String) {
        let _ = self.prompt_tx.send(text);
    }

    /// Cancel the current prompt. The agent will stop and `AgentEvent::Cancelled` is sent.
    pub fn cancel(&self) {
        let _ = self.cancel_tx.send(());
    }
}

async fn init_session(
    tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    mut prompt_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
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

    let (conn, handle_io) = acp::ClientSideConnection::new(
        BrowserClient {
            tx: tx.clone(),
            wake: std::sync::Arc::clone(&wake),
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
    while let Some(text) = prompt_rx.recv().await {
        let prompt_fut = conn.prompt(acp::PromptRequest::new(
            session_id.clone(),
            vec![text.into()],
        ));

        // Use tokio::select! to allow cancellation mid-prompt
        tokio::select! {
            res = prompt_fut => {
                match res {
                    Ok(_) => {
                        let _ = tx.send(AgentEvent::Done);
                        wake();
                    }
                    Err(e) => {
                        let _ = tx.send(AgentEvent::Error(e.to_string()));
                        wake();
                    }
                }
            }
            _ = cancel_rx.recv() => {
                // Cancel received — send cancel notification to agent
                let _ = conn
                    .cancel(acp::CancelNotification::new(session_id.clone()))
                    .await;
                // Send cancelled event
                let _ = tx.send(AgentEvent::Cancelled);
                wake();
            }
        }
    }

    Ok(())
}
