//! Shells behind the terminal panel (⌘`): one login shell per terminal tab,
//! each on its own pseudo-terminal.
//!
//! Output reaches the panel by long poll on `octoweb-term://localhost/<id>`
//! rather than `evaluate_script`, which leaks a JS context per call
//! (wry#1489) — a busy shell would call it hundreds of times a second. The
//! panel keeps one read parked per shell; a reader thread buffers output and
//! wakes the event loop to answer it, and stops reading while `MAX_PENDING`
//! bytes sit unread, so a flood blocks the shell on a full PTY instead of
//! growing memory. Keystrokes go through a writer thread: a program that
//! isn't reading would otherwise block the main thread on a full PTY.

use std::collections::HashMap;
use std::ffi::{CStr, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Condvar, Mutex};

use serde::Deserialize;
use tao::event_loop::EventLoopProxy;
use wry::http::Response;
use wry::RequestAsyncResponder;

use crate::AppEvent;

/// Unread output a shell may buffer before its reader stops reading.
const MAX_PENDING: usize = 4 << 20;
const READ_CHUNK: usize = 64 << 10;

/// Messages from the panel page.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Open {
        id: u32,
        cols: u16,
        rows: u16,
    },
    Input {
        id: u32,
        data: Keys,
    },
    Resize {
        id: u32,
        cols: u16,
        rows: u16,
    },
    Close {
        id: u32,
    },
    OpenUrl {
        url: String,
    },
    Hide,
    Fullscreen,
    /// Live panel height while its top edge is dragged, in logical points.
    ResizePanel {
        height: u32,
    },
    /// Final panel height, to persist.
    ResizePanelEnd {
        height: u32,
    },
    ResizePanelReset,
}

/// Terminal input. Its Debug output is redacted: it carries whatever is
/// typed, passwords included.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Keys(pub String);

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(..)")
    }
}

struct Stream {
    pipe: Mutex<Pipe>,
    drained: Condvar,
}

#[derive(Default)]
struct Pipe {
    bytes: Vec<u8>,
    /// The panel's parked read, answered once there is output or the shell is gone.
    read: Option<RequestAsyncResponder>,
    /// No more output will come: the shell exited or its tab closed.
    done: bool,
}

struct Shell {
    child: Arc<Mutex<Child>>,
    master: OwnedFd,
    input: mpsc::Sender<Vec<u8>>,
    stream: Arc<Stream>,
}

pub struct Terminals {
    shells: HashMap<u32, Shell>,
    /// Shared with the `octoweb-term` protocol handler.
    streams: Arc<Mutex<HashMap<u32, Arc<Stream>>>>,
    proxy: EventLoopProxy<AppEvent>,
}

impl Terminals {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            shells: HashMap::new(),
            streams: Arc::new(Mutex::new(HashMap::new())),
            proxy,
        }
    }

    /// Handler for `octoweb-term://localhost/<id>`, the panel's read of a
    /// shell's output: 200 carries output, 410 means the shell is gone.
    pub fn output_protocol(
        &self,
    ) -> impl Fn(wry::WebViewId, wry::http::Request<Vec<u8>>, RequestAsyncResponder) + 'static {
        let streams = Arc::clone(&self.streams);
        move |_: wry::WebViewId,
              request: wry::http::Request<Vec<u8>>,
              read: RequestAsyncResponder| {
            let id = request.uri().path().trim_start_matches('/').parse::<u32>();
            let stream = id
                .ok()
                .and_then(|id| streams.lock().unwrap().get(&id).cloned());
            match stream {
                Some(stream) => serve(&stream, read),
                None => respond(read, 404, Vec::new()),
            }
        }
    }

    /// Start a login shell for terminal `id` at `cols`×`rows`.
    pub fn open(&mut self, id: u32, cols: u16, rows: u16) -> io::Result<()> {
        let (master, slave) = open_pty(cols, rows)?;
        // The Command, and with it the parent's copies of the slave, drops
        // here — otherwise the reader would never see the shell hang up.
        let child = login_shell_command(slave)?.spawn()?;
        let child = Arc::new(Mutex::new(child));
        let stream = Arc::new(Stream {
            pipe: Mutex::default(),
            drained: Condvar::new(),
        });
        let (input, keys) = mpsc::channel::<Vec<u8>>();
        let mut writer = File::from(master.try_clone()?);
        let reader = File::from(master.try_clone()?);
        std::thread::Builder::new()
            .name("terminal-write".into())
            .spawn(move || {
                for bytes in keys {
                    if writer.write_all(&bytes).is_err() {
                        break;
                    }
                }
            })
            .expect("failed to spawn terminal writer thread");
        std::thread::Builder::new()
            .name("terminal-read".into())
            .spawn({
                let stream = Arc::clone(&stream);
                let child = Arc::clone(&child);
                let proxy = self.proxy.clone();
                move || pump(id, reader, &stream, &child, &proxy)
            })
            .expect("failed to spawn terminal reader thread");
        self.streams.lock().unwrap().insert(id, Arc::clone(&stream));
        self.shells.insert(
            id,
            Shell {
                child,
                master,
                input,
                stream,
            },
        );
        Ok(())
    }

    pub fn write(&self, id: u32, bytes: Vec<u8>) {
        if let Some(shell) = self.shells.get(&id) {
            // Fails only once the shell is gone, when its tab is closing anyway.
            let _ = shell.input.send(bytes);
        }
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) {
        if let Some(shell) = self.shells.get(&id) {
            let size = window_size(cols, rows);
            // SAFETY: TIOCSWINSZ reads one winsize. The kernel then sends
            // SIGWINCH to the foreground job.
            unsafe {
                libc::ioctl(
                    shell.master.as_raw_fd(),
                    libc::TIOCSWINSZ,
                    &size as *const libc::winsize,
                )
            };
        }
    }

    /// Answer terminal `id`'s parked read once its reader saw output or exit.
    pub fn flush(&self, id: u32) {
        if let Some(shell) = self.shells.get(&id) {
            let parked = shell.stream.pipe.lock().unwrap().read.take();
            if let Some(read) = parked {
                serve(&shell.stream, read);
            }
        }
    }

    /// Hang up terminal `id`'s shell, as closing a Terminal.app tab does.
    pub fn close(&mut self, id: u32) {
        let Some(shell) = self.shells.remove(&id) else {
            return;
        };
        self.streams.lock().unwrap().remove(&id);
        let parked = {
            let mut pipe = shell.stream.pipe.lock().unwrap();
            pipe.done = true;
            pipe.read.take()
        };
        shell.stream.drained.notify_all();
        if let Some(read) = parked {
            respond(read, 410, Vec::new());
        }
        let mut child = shell.child.lock().unwrap();
        // Not reaped yet, so the pid is still this shell's.
        if matches!(child.try_wait(), Ok(None)) {
            // SAFETY: plain kill(2) on our own child.
            unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGHUP) };
        }
    }
}

/// Answer the panel's read with pending output, or park it until there is some.
fn serve(stream: &Stream, read: RequestAsyncResponder) {
    let mut pipe = stream.pipe.lock().unwrap();
    if pipe.bytes.is_empty() && !pipe.done {
        pipe.read = Some(read);
        return;
    }
    let bytes = std::mem::take(&mut pipe.bytes);
    drop(pipe);
    stream.drained.notify_all();
    // Nothing pending past this point means the shell is gone.
    let status = if bytes.is_empty() { 410 } else { 200 };
    respond(read, status, bytes);
}

fn respond(read: RequestAsyncResponder, status: u16, body: Vec<u8>) {
    read.respond(
        Response::builder()
            .status(status)
            .header("Access-Control-Allow-Origin", "*")
            .body(body)
            .unwrap(),
    );
}

/// Move a shell's output into its stream until the shell and everything it
/// started have closed the terminal, when reading fails with EIO.
fn pump(
    id: u32,
    mut master: File,
    stream: &Stream,
    child: &Mutex<Child>,
    proxy: &EventLoopProxy<AppEvent>,
) {
    let mut chunk = vec![0; READ_CHUNK];
    loop {
        let n = match master.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let mut pipe = stream.pipe.lock().unwrap();
        // A closed tab's shell is still drained until it exits, or it would
        // block writing to a terminal nobody reads.
        if pipe.done {
            continue;
        }
        if pipe.bytes.is_empty() && pipe.read.is_some() {
            let _ = proxy.send_event(AppEvent::TerminalOutput(id));
        }
        pipe.bytes.extend_from_slice(&chunk[..n]);
        while pipe.bytes.len() >= MAX_PENDING && !pipe.done {
            pipe = stream.drained.wait(pipe).unwrap();
        }
    }
    let parked = {
        let mut pipe = stream.pipe.lock().unwrap();
        pipe.done = true;
        pipe.read.is_some()
    };
    if parked {
        let _ = proxy.send_event(AppEvent::TerminalOutput(id));
    }
    // Reaped only now: until then the pid can't be reused, which keeps the
    // SIGHUP in `close` on this shell.
    let _ = child.lock().unwrap().wait();
}

fn open_pty(cols: u16, rows: u16) -> io::Result<(OwnedFd, OwnedFd)> {
    let (mut master, mut slave) = (-1, -1);
    let mut size = window_size(cols, rows);
    // SAFETY: valid out-pointers; name and termios may be null.
    let status = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openpty just returned these two descriptors to us.
    let fds = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // Neither may reach the shell except as its stdio: a stray master keeps
    // the terminal open after its tab closes.
    for fd in [&fds.0, &fds.1] {
        // SAFETY: fcntl on a descriptor we own.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(fds)
}

fn window_size(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// The account's shell started the way Terminal.app starts it: as a login
/// shell (`-zsh` argv[0]) in the home directory.
fn login_shell_command(slave: OwnedFd) -> io::Result<Command> {
    // SAFETY: getpwuid's record is only read here, on the main thread, and
    // copied out before returning.
    let (shell, home) = unsafe {
        let user = libc::getpwuid(libc::getuid());
        if user.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no account record for this user",
            ));
        }
        let path = |field: *const libc::c_char| {
            PathBuf::from(OsStr::from_bytes(CStr::from_ptr(field).to_bytes()))
        };
        (path((*user).pw_shell), path((*user).pw_dir))
    };
    let name = shell
        .file_name()
        .ok_or_else(|| io::Error::other("the login shell path has no file name"))?;
    let mut login_name = OsString::from("-");
    login_name.push(name);

    let mut cmd = Command::new(&shell);
    cmd.arg0(login_name)
        .current_dir(home)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("TERM_PROGRAM", "octoweb")
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    // Apps started from Finder get no LANG, and shells then mangle non-ASCII.
    if std::env::var_os("LANG").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            // A session of its own with the PTY as controlling terminal, so
            // job control and ⌃C reach the foreground program.
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(cmd)
}
