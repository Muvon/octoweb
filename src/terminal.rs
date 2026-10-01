//! Shells behind the terminal panel (⌘`): one login shell per terminal pane,
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
//!
//! Shells outlive the page: each keeps its recent output, so a reloaded panel
//! or a workspace mounted again reattaches and replays it, and a restart
//! starts the shell again in its last directory under that output.

use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Condvar, Mutex};

use serde::Deserialize;
use tao::event_loop::EventLoopProxy;
use wry::http::Response;
use wry::RequestAsyncResponder;

use crate::config::SavedShell;
use crate::AppEvent;

/// Unread output a shell may buffer before its reader stops reading.
const MAX_PENDING: usize = 4 << 20;
const READ_CHUNK: usize = 64 << 10;
/// Output a shell keeps for replay, about xterm.js's 10 000 lines of scrollback.
const HISTORY_BYTES: usize = 1 << 20;
/// Written under a restored pane's old output, before its new shell starts.
const RESTORED_MARK: &str = "\x1b[!p\r\n\x1b[2m── restored ──\x1b[0m\r\n";

/// Messages from the panel page.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// The panel page loaded: a first start or a reload.
    Ready,
    /// Workspace `ws`'s tabs and splits changed.
    Layout {
        ws: String,
        layout: serde_json::Value,
    },
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
    /// The panel took the keyboard; `title` is the focused pane's shell title.
    Focus {
        title: String,
    },
    /// The panel lost the keyboard.
    Blur,
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
    /// The last `HISTORY_BYTES` of output, replayed into a pane attached anew.
    history: VecDeque<u8>,
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
    /// Panes saved by the last run, started again once the panel opens them.
    restored: HashMap<u32, SavedShell>,
    /// Above every pane id in use or saved.
    next_id: u32,
}

impl Terminals {
    pub fn new(
        proxy: EventLoopProxy<AppEvent>,
        restored: HashMap<u32, SavedShell>,
        next_id: u32,
    ) -> Self {
        Self {
            shells: HashMap::new(),
            streams: Arc::new(Mutex::new(HashMap::new())),
            proxy,
            restored,
            next_id,
        }
    }

    /// Where the panel numbers new panes from.
    pub fn next_id(&self) -> u32 {
        self.next_id
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

    /// Attach terminal `id` at `cols`×`rows`. A shell still running for it (the
    /// panel reloaded) replays its output into the new pane; otherwise a login
    /// shell starts, in the directory and under the output it was saved with.
    pub fn open(&mut self, id: u32, cols: u16, rows: u16) -> io::Result<()> {
        self.next_id = self.next_id.max(id + 1);
        if let Some(shell) = self.shells.get(&id) {
            let stale = {
                let mut pipe = shell.stream.pipe.lock().unwrap();
                pipe.bytes = pipe.history.iter().copied().collect();
                pipe.read.take()
            };
            shell.stream.drained.notify_all();
            // The read the previous page left parked; that page is gone.
            if let Some(read) = stale {
                respond(read, 409, Vec::new());
            }
            self.resize(id, cols, rows);
            return Ok(());
        }
        let restored = self.restored.remove(&id).unwrap_or_default();
        let (master, slave) = open_pty(cols, rows)?;
        // The Command, and with it the parent's copies of the slave, drops
        // here — otherwise the reader would never see the shell hang up.
        let child = login_shell_command(slave, restored.cwd.as_deref())?.spawn()?;
        let child = Arc::new(Mutex::new(child));
        let mut pipe = Pipe::default();
        if !restored.history.is_empty() {
            remember(&mut pipe.history, &restored_replay(restored.history));
            pipe.bytes = pipe.history.iter().copied().collect();
        }
        let stream = Arc::new(Stream {
            pipe: Mutex::new(pipe),
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
        self.restored.remove(&id);
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

    /// Directory and recent output of each pane in `ids`, to restore next
    /// launch. A pane not opened since this launch keeps what it was saved with.
    pub fn snapshot(&self, ids: &[u32]) -> HashMap<u32, SavedShell> {
        ids.iter()
            .filter_map(|&id| {
                let saved = match self.shells.get(&id) {
                    Some(shell) => SavedShell {
                        cwd: process_cwd(shell.child.lock().unwrap().id()),
                        history: shell
                            .stream
                            .pipe
                            .lock()
                            .unwrap()
                            .history
                            .iter()
                            .copied()
                            .collect(),
                    },
                    None => self.restored.get(&id)?.clone(),
                };
                Some((id, saved))
            })
            .collect()
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

/// Append to a shell's history, dropping the oldest output past
/// `HISTORY_BYTES` through a line break, so a replay starts on a whole line.
fn remember(history: &mut VecDeque<u8>, bytes: &[u8]) {
    history.extend(bytes);
    if history.len() > HISTORY_BYTES {
        history.drain(..history.len() - HISTORY_BYTES);
        let line = history
            .iter()
            .position(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        history.drain(..line);
    }
}

/// A saved pane's output to replay above its new shell. A program that was
/// still on the alternate screen (vim, less) is left, so the new prompt
/// comes up under the scrollback.
fn restored_replay(mut history: Vec<u8>) -> Vec<u8> {
    let last = |seq: &[u8]| history.windows(seq.len()).rposition(|w| w == seq);
    let on_alternate_screen = last(b"\x1b[?1049h") > last(b"\x1b[?1049l");
    if on_alternate_screen {
        history.extend_from_slice(b"\x1b[?1049l");
    }
    history.extend_from_slice(RESTORED_MARK.as_bytes());
    history
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
        remember(&mut pipe.history, &chunk[..n]);
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
/// shell (`-zsh` argv[0]), in `cwd` when it still exists, else the home
/// directory.
fn login_shell_command(slave: OwnedFd, cwd: Option<&Path>) -> io::Result<Command> {
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
        .current_dir(cwd.filter(|dir| dir.is_dir()).unwrap_or(&home))
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

/// Current directory of process `pid`.
fn process_cwd(pid: u32) -> Option<PathBuf> {
    // SAFETY: plain C struct; all-zero is a valid value.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel NUL-terminates the path within its buffer.
    let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
    Some(PathBuf::from(OsStr::from_bytes(path.to_bytes())))
}

/// Pane ids in a panel layout (see `__termSpace` in `terminal_html.rs`).
pub fn layout_panes(layout: &serde_json::Value) -> Vec<u32> {
    fn walk(node: &serde_json::Value, ids: &mut Vec<u32>) {
        match node {
            serde_json::Value::Object(map) => {
                if let Some(id) = map.get("pane").and_then(serde_json::Value::as_u64) {
                    ids.push(id as u32);
                }
                map.values().for_each(|child| walk(child, ids));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|child| walk(child, ids)),
            _ => {}
        }
    }
    let mut ids = Vec::new();
    walk(layout, &mut ids);
    ids
}

/// Ghostty config keys holding a single colour, with their xterm.js theme key.
const GHOSTTY_COLOR_KEYS: [(&str, &str); 6] = [
    ("background", "background"),
    ("foreground", "foreground"),
    ("cursor-color", "cursor"),
    ("cursor-text", "cursorAccent"),
    ("selection-background", "selectionBackground"),
    ("selection-foreground", "selectionForeground"),
];

/// xterm.js theme keys for palette entries 0–15.
const ANSI_THEME_KEYS: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightMagenta",
    "brightCyan",
    "brightWhite",
];

/// The user's own terminal colours, from Ghostty's resolved config —
/// `+show-config` has already applied `theme =` and the defaults. Shaped
/// `{dark, theme, options}`: `theme` is an xterm.js theme, `dark` the panel
/// appearance it stands in for, `options` Ghostty's bold brightening and
/// minimum contrast. `None` without Ghostty; the panel then keeps its built-in
/// light and dark palettes.
pub fn shell_theme() -> Option<serde_json::Value> {
    let cli = crate::macos::ghostty_cli()?;
    let output = match Command::new(&cli)
        .args(["+show-config", "--changes-only=false"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            tracing::warn!(
                status = %output.status,
                stderr = %String::from_utf8_lossy(&output.stderr),
                "ghostty +show-config failed"
            );
            return None;
        }
        Err(e) => {
            tracing::warn!(error = %e, cli = %cli.display(), "ghostty +show-config did not start");
            return None;
        }
    };
    let theme = parse_ghostty_config(&String::from_utf8_lossy(&output.stdout));
    if theme.is_none() {
        tracing::warn!("ghostty +show-config printed no background or foreground");
    }
    theme
}

fn parse_ghostty_config(config: &str) -> Option<serde_json::Value> {
    let mut theme = serde_json::Map::new();
    let mut bold_is_bright = false;
    let mut minimum_contrast: f64 = 1.0;
    for line in config.lines() {
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        match key {
            "palette" => {
                let Some((index, color)) = value.split_once('=') else {
                    continue;
                };
                let name = index
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| ANSI_THEME_KEYS.get(i));
                if let (Some(name), Some(color)) = (name, hex_color(color)) {
                    theme.insert((*name).into(), color.into());
                }
            }
            "bold-color" => bold_is_bright = value == "bright",
            "minimum-contrast" => {
                if let Ok(ratio) = value.parse() {
                    minimum_contrast = ratio;
                }
            }
            _ => {
                // Ghostty-only values such as `cell-foreground` have no xterm.js
                // equivalent; leaving the key out lets xterm.js derive it.
                if let (Some((_, name)), Some(color)) = (
                    GHOSTTY_COLOR_KEYS
                        .iter()
                        .find(|(ghostty, _)| *ghostty == key),
                    hex_color(value),
                ) {
                    theme.insert((*name).into(), color.into());
                }
            }
        }
    }
    let background = theme.get("background")?.as_str()?;
    if !theme.contains_key("foreground") {
        return None;
    }
    // The same cut Ghostty makes to pick a light or dark window.
    let dark = perceived_luminance(background) <= 0.5;
    Some(serde_json::json!({
        "dark": dark,
        "theme": theme,
        "options": {
            "drawBoldTextInBrightColors": bold_is_bright,
            "minimumContrastRatio": minimum_contrast,
        },
    }))
}

/// `value` when it is a `#rrggbb` colour.
fn hex_color(value: &str) -> Option<&str> {
    let digits = value.strip_prefix('#')?;
    (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit())).then_some(value)
}

/// Perceived luminance (0–1) of a `#rrggbb` colour.
fn perceived_luminance(hex: &str) -> f64 {
    let channel = |i: usize| {
        f64::from(u8::from_str_radix(&hex[i..i + 2], 16).expect("checked by hex_color")) / 255.0
    };
    0.299 * channel(1) + 0.587 * channel(3) + 0.114 * channel(5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_the_newest_output_from_a_line_start() {
        let mut history = VecDeque::new();
        remember(&mut history, &vec![b'x'; HISTORY_BYTES]);
        assert_eq!(history.len(), HISTORY_BYTES);
        // Over the cap: the cut lands mid-line, so the rest of that line goes too.
        remember(&mut history, b"\nnew");
        assert_eq!(history.iter().copied().collect::<Vec<u8>>(), b"new");
    }

    #[test]
    fn restored_replay_leaves_the_alternate_screen() {
        let replay = restored_replay(b"$ vim\x1b[?1049hbuffer".to_vec());
        assert!(replay.starts_with(b"$ vim\x1b[?1049hbuffer\x1b[?1049l"));
        assert!(replay.ends_with(RESTORED_MARK.as_bytes()));

        let replay = restored_replay(b"\x1b[?1049hvim\x1b[?1049l$ ".to_vec());
        assert_eq!(
            replay,
            [
                &b"\x1b[?1049hvim\x1b[?1049l$ "[..],
                RESTORED_MARK.as_bytes()
            ]
            .concat()
        );
    }

    #[test]
    fn layout_panes_walks_tabs_and_splits() {
        let layout = serde_json::json!({
            "active": 0,
            "tabs": [
                { "focused": 2, "root": { "split": "row", "children": [
                    { "pane": 1 },
                    { "split": "column", "children": [{ "pane": 2 }, { "pane": 5 }] },
                ] } },
                { "focused": 3, "root": { "pane": 3 } },
            ],
        });
        let mut ids = layout_panes(&layout);
        ids.sort_unstable();
        assert_eq!(ids, [1, 2, 3, 5]);
    }

    #[test]
    fn current_directory_of_this_process() {
        assert_eq!(
            process_cwd(std::process::id()),
            Some(std::env::current_dir().unwrap())
        );
    }

    #[test]
    fn ghostty_config_maps_to_xterm_options() {
        let theme = parse_ghostty_config(
            "font-size = 16\n\
             background = #1e2326\n\
             foreground = #f1f2f3\n\
             cursor-color = #cdd4d9\n\
             cursor-text = \n\
             selection-background = #515e61\n\
             selection-foreground = cell-foreground\n\
             palette = 0=#000000\n\
             palette = 12=#b9ddfc\n\
             palette = 16=#000000\n\
             bold-color = bright\n\
             minimum-contrast = 1.1\n",
        )
        .unwrap();
        assert_eq!(
            theme,
            serde_json::json!({
                "dark": true,
                "theme": {
                    "background": "#1e2326",
                    "foreground": "#f1f2f3",
                    "cursor": "#cdd4d9",
                    "selectionBackground": "#515e61",
                    "black": "#000000",
                    "brightBlue": "#b9ddfc",
                },
                "options": {
                    "drawBoldTextInBrightColors": true,
                    "minimumContrastRatio": 1.1,
                },
            })
        );
    }

    #[test]
    fn light_background_and_plain_bold() {
        let theme =
            parse_ghostty_config("background = #f7f7f7\nforeground = #4a4543\nbold-color = \n")
                .unwrap();
        assert_eq!(theme["dark"], false);
        assert_eq!(theme["options"]["drawBoldTextInBrightColors"], false);
        assert_eq!(theme["options"]["minimumContrastRatio"], 1.0);
    }

    #[test]
    fn config_without_background_is_rejected() {
        assert!(parse_ghostty_config("foreground = #ffffff\n").is_none());
    }
}
