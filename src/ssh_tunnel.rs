//! SSH tunnel proxies: for every enabled SSH proxy, keep `ssh -N -D` running on
//! its local port so tabs can use it as a SOCKS5 proxy.
//!
//! One supervisor thread per tunnel restarts ssh with backoff when it exits and
//! reports status to the main loop. Auth is whatever `ssh user@server` does in a
//! terminal — keys, ssh-agent, `~/.ssh/config` — plus an optional password kept
//! in the Keychain. ssh reads it through `SSH_ASKPASS` pointed at this same
//! binary (see `askpass`), so the secret never lands in argv, env or on disk.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use security_framework::passwords;
use tao::event_loop::EventLoopProxy;

use crate::config::{ProxyKind, ProxyRule};
use crate::AppEvent;

/// Set on the ssh child. An octoweb started with it is ssh's askpass helper
/// for the proxy whose hex id it holds.
pub const ASKPASS_ENV: &str = "OCTOWEB_SSH_ASKPASS";
const KEYCHAIN_SERVICE: &str = "octoweb.ssh-proxy";
const POLL: Duration = Duration::from_millis(500);
const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// macOS lets unprivileged apps bind lower ports only on 0.0.0.0, never on
/// 127.0.0.1. Mirrored by the local port field in settings_html.
const MIN_LOCAL_PORT: u16 = 1024;

#[derive(Debug, Clone)]
pub enum Status {
    Connecting,
    Connected,
    Failed(String),
}

impl Status {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Status::Connecting => serde_json::json!({ "state": "connecting" }),
            Status::Connected => serde_json::json!({ "state": "connected" }),
            Status::Failed(message) => serde_json::json!({ "state": "failed", "message": message }),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Spec {
    destination: String,
    port: u16,
    has_password: bool,
}

struct Tunnel {
    spec: Spec,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Tunnel {
    fn stop(&self) {
        // Flag first, then the lock: the supervisor spawns and reports only
        // under this lock after re-checking the flag, so nothing from this
        // tunnel reaches the main loop after stop() returns.
        self.stop.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
            // Reap now so the port is free before a restart binds it again.
            let _ = child.wait();
        }
    }
}

pub struct Tunnels {
    running: HashMap<[u8; 16], Tunnel>,
    proxy: EventLoopProxy<AppEvent>,
}

impl Tunnels {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            running: HashMap::new(),
            proxy,
        }
    }

    /// Run a tunnel for every enabled, valid SSH rule and stop the rest. A
    /// changed server, port or password flag restarts that tunnel. Returns the
    /// ids of tunnels started by this call.
    pub fn sync(&mut self, rules: &[ProxyRule]) -> Vec<[u8; 16]> {
        let wanted: HashMap<[u8; 16], Spec> = rules
            .iter()
            .filter(|r| {
                r.enabled && r.kind == ProxyKind::Ssh && crate::site_proxy::valid_endpoint(r)
            })
            .map(|r| {
                let spec = Spec {
                    destination: r.ssh.clone(),
                    port: r.port,
                    has_password: r.has_password,
                };
                (r.id, spec)
            })
            .collect();
        self.running.retain(|id, tunnel| {
            let keep = wanted.get(id) == Some(&tunnel.spec);
            if !keep {
                tunnel.stop();
            }
            keep
        });
        let mut started = Vec::new();
        for (id, spec) in wanted {
            if let Entry::Vacant(slot) = self.running.entry(id) {
                slot.insert(start(id, spec, self.proxy.clone()));
                started.push(id);
            }
        }
        started
    }

    /// Stop one tunnel so the next `sync` starts it fresh — ssh reads the
    /// password only when it connects.
    pub fn stop(&mut self, id: &[u8; 16]) {
        if let Some(tunnel) = self.running.remove(id) {
            tunnel.stop();
        }
    }

    pub fn is_running(&self, id: &[u8; 16]) -> bool {
        self.running.contains_key(id)
    }

    /// Kill every ssh child. Quit exits the process without running destructors.
    // ponytail: a crash still orphans ssh; its port then fails the next launch's
    // tunnel with "Address already in use" until that ssh is killed.
    pub fn stop_all(&mut self) {
        for tunnel in self.running.values() {
            tunnel.stop();
        }
        self.running.clear();
    }
}

fn start(id: [u8; 16], spec: Spec, proxy: EventLoopProxy<AppEvent>) -> Tunnel {
    let tunnel = Tunnel {
        spec: spec.clone(),
        stop: Arc::new(AtomicBool::new(false)),
        child: Arc::new(Mutex::new(None)),
    };
    let stop = Arc::clone(&tunnel.stop);
    let child = Arc::clone(&tunnel.child);
    std::thread::Builder::new()
        .name("ssh-tunnel".into())
        .spawn(move || supervise(id, &spec, &stop, &child, &proxy))
        .expect("failed to spawn ssh tunnel thread");
    tunnel
}

fn supervise(
    id: [u8; 16],
    spec: &Spec,
    stop: &AtomicBool,
    slot: &Mutex<Option<Child>>,
    proxy: &EventLoopProxy<AppEvent>,
) {
    // Reports go out under `slot` after re-checking `stop` — see Tunnel::stop.
    let report = |status: Status| {
        let _slot = slot.lock().unwrap();
        let live = !stop.load(Ordering::SeqCst);
        if live {
            let _ = proxy.send_event(AppEvent::ProxyStatus(id, status));
        }
        live
    };
    let mut backoff = MIN_BACKOFF;
    while report(Status::Connecting) {
        // The port probe below can't tell whose listener answers, so the port
        // must be free before ssh starts.
        let failure = if spec.port < MIN_LOCAL_PORT {
            // ssh would only report "Could not request local forwarding."
            format!(
                "Local port {} is reserved by macOS. Use {MIN_LOCAL_PORT} or higher.",
                spec.port
            )
        } else if port_open(spec.port) {
            format!("127.0.0.1:{} is already used by another program", spec.port)
        } else {
            match spawn_into(slot, stop, &id, spec) {
                None => return,
                Some(Err(e)) => format!("Could not start ssh: {e}"),
                Some(Ok(stderr)) => {
                    let reader = std::thread::spawn(move || last_line(stderr));
                    let mut connected = false;
                    while child_running(slot) {
                        // ssh opens the -D listener only once it has logged in.
                        if !connected && port_open(spec.port) {
                            connected = true;
                            backoff = MIN_BACKOFF;
                            report(Status::Connected);
                        }
                        std::thread::sleep(POLL);
                    }
                    let line = reader.join().unwrap_or_default();
                    if line.is_empty() {
                        "ssh exited".to_string()
                    } else {
                        line
                    }
                }
            }
        };
        if !report(Status::Failed(failure)) {
            return;
        }
        let resume = Instant::now() + backoff;
        while Instant::now() < resume && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(POLL);
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Spawn ssh into `slot` unless the tunnel was stopped; returns its stderr.
fn spawn_into(
    slot: &Mutex<Option<Child>>,
    stop: &AtomicBool,
    id: &[u8; 16],
    spec: &Spec,
) -> Option<std::io::Result<ChildStderr>> {
    let mut slot = slot.lock().unwrap();
    if stop.load(Ordering::SeqCst) {
        return None;
    }
    Some(
        command(id, spec)
            .and_then(|mut cmd| cmd.spawn())
            .map(|mut child| {
                let stderr = child.stderr.take().expect("ssh stderr is piped");
                *slot = Some(child);
                stderr
            }),
    )
}

/// Whether ssh is still up; reaps it once it has exited.
fn child_running(slot: &Mutex<Option<Child>>) -> bool {
    let mut slot = slot.lock().unwrap();
    let running = slot
        .as_mut()
        .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
    if !running {
        slot.take();
    }
    running
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), POLL).is_ok()
}

/// Drain ssh's stderr — a full pipe would block it — keeping the last line,
/// which is the error worth showing once it exits.
fn last_line(stderr: ChildStderr) -> String {
    BufReader::new(stderr)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .last()
        .unwrap_or_default()
}

fn command(id: &[u8; 16], spec: &Spec) -> std::io::Result<Command> {
    let mut cmd = Command::new("/usr/bin/ssh");
    cmd.arg("-N")
        .arg("-D")
        .arg(format!("127.0.0.1:{}", spec.port))
        .args(["-o", "ExitOnForwardFailure=yes"])
        .args(["-o", "ServerAliveInterval=15"])
        .args(["-o", "ServerAliveCountMax=3"])
        .args(["-o", "ConnectTimeout=15"])
        // Nobody can answer a new-host-key prompt; a changed key still fails.
        .args(["-o", "StrictHostKeyChecking=accept-new"])
        // Always a connection of our own: through a shared ControlMaster the
        // master takes over the -D forward and this ssh exits at once, leaving
        // a tunnel nobody restarts.
        .args(["-o", "ControlMaster=no"])
        .args(["-o", "ControlPath=none"]);
    if spec.has_password {
        cmd.args(["-o", "NumberOfPasswordPrompts=1"])
            .env("SSH_ASKPASS", std::env::current_exe()?)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(ASKPASS_ENV, hex(id));
    } else {
        // Keys and agent only: fail fast instead of waiting on a hidden prompt.
        cmd.args(["-o", "BatchMode=yes"]);
    }
    // `site_proxy::valid_endpoint` rejects a leading '-', so this is never
    // parsed as an option.
    cmd.arg(&spec.destination)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    Ok(cmd)
}

pub fn save_password(id: &[u8; 16], password: &str) -> Result<(), String> {
    passwords::set_generic_password(KEYCHAIN_SERVICE, &hex(id), password.as_bytes())
        .map_err(|e| e.to_string())
}

pub fn forget_password(id: &[u8; 16]) {
    // No stored item is fine — nothing to forget.
    let _ = passwords::delete_generic_password(KEYCHAIN_SERVICE, &hex(id));
}

/// Entry point when ssh runs octoweb as `SSH_ASKPASS`: print the password.
pub fn askpass(id_hex: &str) -> ! {
    match passwords::get_generic_password(KEYCHAIN_SERVICE, id_hex) {
        Ok(password) => {
            let mut out = std::io::stdout();
            let _ = out.write_all(&password);
            let _ = out.write_all(b"\n");
            let _ = out.flush();
            std::process::exit(0)
        }
        Err(_) => std::process::exit(1),
    }
}

pub fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}
