//! The one `octomind server` process every assistant session runs in.
//!
//! Octoweb used to spawn an `octomind acp <tag>` subprocess per chat, per
//! background job. Now a single WebSocket server hosts them all: each session
//! binds with its own working directory and role (see `agent.rs`), so one
//! process serves every workspace's assistants, the learning agent, inline edit
//! and the mail jobs.
//!
//! The server is started lazily by the first session that needs it, on a free
//! loopback port, and restarted the same way if it dies — sessions see their
//! socket close, report an error, and the main loop's reconnect brings them
//! back, which starts a fresh server and resumes each session from disk.
//!
//! Every handshake must carry a per-run secret (`OCTOMIND_SERVER_TOKEN`), so
//! no other local process can drive the agent; browsers are refused by the
//! server's Origin check. The token travels in the environment only.

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The role the server starts under. Sessions name their own role on bind;
/// this one only decides which tap the server resolves at startup.
const SERVER_ROLE: &str = "octoweb:chief";

/// Startup resolves the server role's tap (and may run its dependency
/// scripts), so the first start after an install can take a while.
const READY_TIMEOUT: Duration = Duration::from_secs(120);

/// Where and how to reach the running server.
#[derive(Clone, Debug)]
pub struct Endpoint {
	pub url: String,
	pub token: String,
}

struct Running {
	child: Child,
	endpoint: Endpoint,
}

static SERVER: Mutex<Option<Running>> = Mutex::new(None);

/// The running server, starting it first if there is none or it has died.
/// Blocking — call it off the main thread. Concurrent callers wait for the
/// one start rather than racing their own.
pub fn endpoint() -> anyhow::Result<Endpoint> {
	let mut server = SERVER.lock().unwrap_or_else(|p| p.into_inner());
	if let Some(running) = server.as_mut() {
		match running.child.try_wait() {
			Ok(None) => return Ok(running.endpoint.clone()),
			Ok(Some(status)) => {
				tracing::warn!(%status, "octomind server exited — restarting");
			}
			Err(e) => tracing::warn!(error = %e, "octomind server state unknown — restarting"),
		}
		*server = None;
	}
	let running = start()?;
	let endpoint = running.endpoint.clone();
	*server = Some(running);
	Ok(endpoint)
}

/// Stop the server, if one runs. Called on quit: `ControlFlow::Exit` skips
/// destructors, so nothing else would end the child.
pub fn shutdown() {
	let running = SERVER.lock().unwrap_or_else(|p| p.into_inner()).take();
	if let Some(mut running) = running {
		terminate(&mut running.child);
		let _ = std::fs::remove_file(pid_path());
	}
}

/// SIGTERM, a short grace for octomind to stop its MCP servers, then SIGKILL.
fn terminate(child: &mut Child) {
	let pid = child.id() as libc::pid_t;
	unsafe {
		libc::kill(pid, libc::SIGTERM);
	}
	let deadline = Instant::now() + Duration::from_secs(3);
	while Instant::now() < deadline {
		if matches!(child.try_wait(), Ok(Some(_))) {
			return;
		}
		std::thread::sleep(Duration::from_millis(50));
	}
	let _ = child.kill();
	let _ = child.wait();
}

fn pid_path() -> PathBuf {
	crate::config::base_dir().join("octoweb").join("octomind-server.pid")
}

/// Media the server reads message attachments from. Octoweb writes a pasted
/// image here and hands the server only its opaque id.
pub fn media_dir() -> PathBuf {
	crate::config::base_dir().join("octoweb").join("agent-media")
}

/// A server left behind by a previous run that crashed or was force-quit
/// would otherwise live on forever. Only a process that still is an
/// `octomind server` is touched — the pid may have been reused since.
fn reap_orphan() {
	let path = pid_path();
	let Some(pid) = std::fs::read_to_string(&path)
		.ok()
		.and_then(|s| s.trim().parse::<libc::pid_t>().ok())
	else {
		return;
	};
	let _ = std::fs::remove_file(&path);
	let command = Command::new("ps")
		.args(["-p", &pid.to_string(), "-o", "command="])
		.output()
		.map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
		.unwrap_or_default();
	if is_octomind_server(&command) {
		tracing::info!(pid, "stopping an octomind server left by a previous run");
		unsafe {
			libc::kill(pid, libc::SIGTERM);
		}
	}
}

/// Variables octoweb hands the server for this run only. octomind loads the
/// server directory's `.env` *over* the real environment, so a stale copy saved
/// there — an octomind run that lacked them once prompts on an absent stdin and
/// saves the empty answer — silently wins: the octoweb MCP URL loses its port
/// and every browser tool goes dead. They never belong in that file.
const RUNTIME_ENV: [&str; 4] = [
	"OCTOWEB_MCP_PORT",
	"OCTOWEB_MCP_TOKEN",
	"OCTOMIND_SERVER_TOKEN",
	"OCTOMIND_MEDIA_ROOT",
];

/// Drop `RUNTIME_ENV` entries from `.env` (the shared file every workspace
/// links to), keeping everything else byte for byte.
fn scrub_runtime_env(path: &std::path::Path) {
	let Ok(text) = std::fs::read_to_string(path) else {
		return;
	};
	let (kept, dropped) = strip_runtime_env(&text);
	if dropped.is_empty() {
		return;
	}
	match std::fs::write(path, kept) {
		Ok(()) => tracing::warn!(
			file = %path.display(),
			keys = ?dropped,
			"removed octoweb runtime variables saved in .env"
		),
		Err(e) => tracing::warn!(error = %e, file = %path.display(), "could not clean .env"),
	}
}

fn strip_runtime_env(text: &str) -> (String, Vec<String>) {
	let mut kept = String::with_capacity(text.len());
	let mut dropped = Vec::new();
	for line in text.split_inclusive('\n') {
		let key = line
			.trim_start()
			.trim_start_matches("export ")
			.split('=')
			.next()
			.unwrap_or_default()
			.trim();
		if line.contains('=') && RUNTIME_ENV.contains(&key) {
			dropped.push(key.to_string());
		} else {
			kept.push_str(line);
		}
	}
	(kept, dropped)
}

/// octomind retitles its process (`octomind-server`); `ps` shows that, or the
/// original argv on systems where retitling is unavailable.
fn is_octomind_server(command: &str) -> bool {
	command.trim_start().starts_with("octomind-server") || (command.contains("octomind") && command.contains(" server"))
}

fn start() -> anyhow::Result<Running> {
	reap_orphan();
	crate::agent::ensure_octomind("octomind");

	// The shared directory: the server's own cwd and `--sandbox` root, and the
	// place it loads `.env` (the octohub key) from. Sessions run in their own
	// workspace directory under the same writable tree.
	let root = crate::agent_workspace::prepare(crate::workspace::DEFAULT_WORKSPACE_ID, SERVER_ROLE)?;
	scrub_runtime_env(&root.join(".env"));
	let media = media_dir();
	std::fs::create_dir_all(&media)?;
	// `OCTOWEB_OCTOMIND_TOKEN` pins the secret, so whoever launched octoweb can
	// attach a debug client (websocat, a script) to the app's own server.
	let token = std::env::var("OCTOWEB_OCTOMIND_TOKEN")
		.ok()
		.filter(|t| !t.trim().is_empty())
		.unwrap_or_else(crate::mcp::random_token);

	let mut child = Command::new("octomind")
		.args(["server", SERVER_ROLE, "--host", "127.0.0.1", "--port", "0", "--sandbox"])
		.current_dir(&root)
		.env("OCTOMIND_SERVER_TOKEN", &token)
		.env("OCTOMIND_MEDIA_ROOT", &media)
		// The `octoweb` tap capability templates both into its MCP server
		// entry: our port, and the token that tells our MCP server the call
		// comes from this agent server (the calling session is in `_meta`).
		.env("OCTOWEB_MCP_PORT", crate::mcp::port().to_string())
		.env("OCTOWEB_MCP_TOKEN", crate::mcp::agent_server_token())
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|e| {
			if e.kind() == std::io::ErrorKind::NotFound {
				anyhow::anyhow!(
					"The AI agent (octomind) isn't installed.\n\
                     Install it with:\n\
                     curl -fsSL https://raw.githubusercontent.com/muvon/octomind/master/install.sh | bash\n\
                     then set an API key (e.g. OPENROUTER_API_KEY) and reopen this sidebar."
				)
			} else {
				anyhow::anyhow!("failed to start octomind server: {e}")
			}
		})?;

	let _ = std::fs::write(pid_path(), child.id().to_string());

	// stderr carries startup failures (a tap that fails to resolve, a bad
	// config); keep its tail for the error and drain it so it never blocks.
	let stderr_tail = std::sync::Arc::new(Mutex::new(String::new()));
	if let Some(stderr) = child.stderr.take() {
		let tail = std::sync::Arc::clone(&stderr_tail);
		std::thread::spawn(move || {
			let mut reader = BufReader::new(stderr);
			let mut buf = [0u8; 4096];
			while let Ok(n) = reader.read(&mut buf) {
				if n == 0 {
					break;
				}
				let mut tail = tail.lock().unwrap_or_else(|p| p.into_inner());
				tail.push_str(&String::from_utf8_lossy(&buf[..n]));
				if tail.len() > 4096 {
					let cut = tail.len() - 4096;
					let cut = (cut..tail.len()).find(|i| tail.is_char_boundary(*i)).unwrap_or(0);
					tail.drain(..cut);
				}
			}
		});
	}

	// The bound address is printed once the listener is up. Keep reading
	// stdout afterwards: a full pipe would block the server's next print.
	let (addr_tx, addr_rx) = std::sync::mpsc::channel::<String>();
	let stdout = child.stdout.take().expect("piped stdout");
	std::thread::spawn(move || {
		let mut addr_tx = Some(addr_tx);
		for line in BufReader::new(stdout).lines().map_while(Result::ok) {
			if let Some(addr) = line.split("ws://").nth(1) {
				if let Some(tx) = addr_tx.take() {
					let _ = tx.send(addr.trim().to_string());
				}
			}
			tracing::debug!(target: "octomind_server", "{line}");
		}
	});

	let started = Instant::now();
	loop {
		match addr_rx.recv_timeout(Duration::from_millis(200)) {
			Ok(addr) => {
				// Versions before the session-server protocol (per-session role,
				// cancel, token) echo the requested `--port 0` instead of the
				// bound port. Say so instead of failing to connect.
				if addr.ends_with(":0") {
					terminate(&mut child);
					let _ = std::fs::remove_file(pid_path());
					anyhow::bail!("octomind is too old for this octoweb — update it (brew upgrade octomind)");
				}
				let endpoint = Endpoint {
					url: format!("ws://{addr}/"),
					token,
				};
				tracing::info!(url = %endpoint.url, pid = child.id(), "octomind server ready");
				return Ok(Running { child, endpoint });
			}
			Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
			Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
				// stdout closed before the address: the process is exiting.
				std::thread::sleep(Duration::from_millis(200));
			}
		}
		if let Ok(Some(status)) = child.try_wait() {
			let _ = std::fs::remove_file(pid_path());
			let tail = stderr_tail.lock().unwrap_or_else(|p| p.into_inner()).clone();
			anyhow::bail!("octomind server exited during startup ({status}){}", last_lines(&tail));
		}
		if started.elapsed() > READY_TIMEOUT {
			terminate(&mut child);
			let _ = std::fs::remove_file(pid_path());
			anyhow::bail!("octomind server did not start within {}s", READY_TIMEOUT.as_secs());
		}
	}
}

/// The last few non-empty lines of a captured stream, as an error suffix.
fn last_lines(text: &str) -> String {
	let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
	if lines.is_empty() {
		return String::new();
	}
	let start = lines.len().saturating_sub(5);
	format!(": {}", lines[start..].join(" | "))
}

#[cfg(test)]
mod tests {
	#[test]
	fn runtime_variables_are_stripped_from_env_and_the_rest_kept() {
		let text =
			"OCTOHUB_API_KEY=abc\nOCTOWEB_MCP_PORT=\nexport OCTOWEB_MCP_TOKEN=x\n# OCTOWEB_MCP_PORT note\nOTHER=1";
		let (kept, dropped) = super::strip_runtime_env(text);
		assert_eq!(kept, "OCTOHUB_API_KEY=abc\n# OCTOWEB_MCP_PORT note\nOTHER=1");
		assert_eq!(dropped, ["OCTOWEB_MCP_PORT", "OCTOWEB_MCP_TOKEN"]);
		let (same, none) = super::strip_runtime_env("A=1\n");
		assert_eq!((same.as_str(), none.len()), ("A=1\n", 0));
	}

	#[test]
	fn only_an_octomind_server_process_is_reaped() {
		assert!(super::is_octomind_server("octomind-server"));
		assert!(super::is_octomind_server(
			"/usr/local/bin/octomind server octoweb:chief --port 0"
		));
		assert!(!super::is_octomind_server("octomind-acp octoweb:chief"));
		assert!(!super::is_octomind_server("octomind-run developer:general"));
		assert!(!super::is_octomind_server("/usr/sbin/httpd"));
	}

	#[test]
	fn startup_errors_keep_only_the_last_lines() {
		assert_eq!(super::last_lines(""), "");
		assert_eq!(super::last_lines("\n  \n"), "");
		let text = (1..=8).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
		assert_eq!(super::last_lines(&text), ": line 4 | line 5 | line 6 | line 7 | line 8");
	}
}
