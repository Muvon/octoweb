//! Isolated browser profiles — each workspace owns its own tabs/history
//! (`TabManager`), its own tab-id → `WebView` map, and its own AI sidebar
//! session list (`acp_sessions`), so tab ids and ACP session ids only need
//! to be unique within a workspace... except ACP session ids are still
//! minted from a process-wide counter (see `main::next_acp_session_id`) —
//! `login_pollers` in main.rs is a flat `HashMap<u64, ..>` with no workspace
//! scoping, so per-workspace-only ids could collide there. Non-default
//! workspaces also get a dedicated `WKWebsiteDataStore` (via `data_store_id`)
//! so cookies, localStorage, and cache never leak between workspaces.
//!
//! No switcher UI in stage 1 — `main.rs` always operated on
//! `WorkspaceManager::active()` until stage 2 added switching.

use crate::browser::TabManager;
use crate::quickslots::QuickSlots;
use crate::AcpSession;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use wry::WebView;

const DEFAULT_COLOR: &str = "#7C5CFF";

/// Id of the always-present first workspace. Also what an MCP caller that
/// sends no workspace token is routed to.
pub const DEFAULT_WORKSPACE_ID: &str = "default";

pub struct Workspace {
    pub id: String,
    pub name: String,
    pub color: String,
    /// `None` = WebKit's default persistent data store. Only the migrated
    /// "Default" workspace uses this, so existing user cookies survive
    /// upgrade. Every workspace created via `create()` gets `Some(id)`.
    pub data_store_id: Option<[u8; 16]>,
    pub tabs: Arc<Mutex<TabManager>>,
    pub webviews: HashMap<usize, WebView>,
    /// AI sidebar sessions belonging to this workspace. Empty until main.rs
    /// populates it at startup (or via `CreateWorkspace`) — constructing an
    /// `AcpSession` needs `acp::AcpHandle::connect`/the wake-proxy, neither
    /// of which this module has access to.
    pub acp_sessions: Vec<AcpSession>,
    /// Which of `acp_sessions` is foreground in the sidebar tab strip.
    pub acp_active_session_id: u64,
    /// Most-recently-used tab id order for this workspace, walked by
    /// Ctrl+P/Ctrl+N (`PrevTab`/`NextTab`). Per-workspace, not just
    /// per-workspace-unique-id-safe: unlike a keyed map, a shared list's
    /// neighboring entries would mix tabs from whichever workspace happened
    /// to be active when each was pushed, picking the "previous" tab from
    /// the wrong workspace.
    pub mru: Vec<usize>,
    /// Pinned quick-slots (⌘1–⌘0) for this workspace. Seeded by main.rs at
    /// startup from `quickslots::load_all()`.
    pub quick_slots: QuickSlots,
    /// URLs of tabs closed in this workspace, most recent last, popped by ⌘⇧T.
    /// Per-workspace for the same reason `mru` is: reopening must not
    /// resurrect a tab into a workspace it never belonged to. Deliberately
    /// not persisted — session restore already brings back the tabs that were
    /// open at quit.
    pub closed_tabs: Vec<String>,
    /// This workspace's MCP token. Agents spawned here send it in
    /// `mcp::WORKSPACE_HEADER`, so their tool calls act on this workspace's
    /// tabs no matter which workspace is on screen.
    pub mcp_token: Option<String>,
}

impl Workspace {
    pub fn new(
        id: String,
        name: String,
        color: String,
        data_store_id: Option<[u8; 16]>,
        max_history: usize,
    ) -> Self {
        Self {
            id,
            name,
            color,
            data_store_id,
            tabs: Arc::new(Mutex::new(TabManager::new(max_history))),
            webviews: HashMap::new(),
            acp_sessions: Vec::new(),
            acp_active_session_id: 0,
            mru: Vec::new(),
            quick_slots: Default::default(),
            closed_tabs: Vec::new(),
            mcp_token: None,
        }
    }
}

pub struct WorkspaceManager {
    workspaces: Vec<Workspace>,
    active_index: usize,
    max_history: usize,
    /// Used to seed the one initial tab of a workspace created via `create()`.
    home_page: String,
}

impl WorkspaceManager {
    /// Fresh install (no persisted session): a single "Default" workspace on
    /// WebKit's default persistent data store.
    pub fn new_default(max_history: usize, home_page: String) -> Self {
        Self {
            workspaces: vec![Workspace::new(
                DEFAULT_WORKSPACE_ID.to_string(),
                "Default".to_string(),
                DEFAULT_COLOR.to_string(),
                None,
                max_history,
            )],
            active_index: 0,
            max_history,
            home_page,
        }
    }

    /// Rebuild from persisted/migrated session state.
    pub fn from_workspaces(
        workspaces: Vec<Workspace>,
        active_id: &str,
        max_history: usize,
        home_page: String,
    ) -> Self {
        let active_index = workspaces
            .iter()
            .position(|w| w.id == active_id)
            .unwrap_or(0);
        Self {
            workspaces,
            active_index,
            max_history,
            home_page,
        }
    }

    pub fn active(&self) -> &Workspace {
        &self.workspaces[self.active_index]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_index]
    }

    pub fn list(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn active_index(&self) -> usize {
        self.active_index
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.workspaces.iter().position(|w| w.id == id)
    }

    /// Workspace owning tab `id`. Per-webview callbacks (title, URL, popup,
    /// crash) arrive with just a tab id and must not assume it belongs to the
    /// workspace on screen — an agent drives tabs in background workspaces.
    /// Tab ids are process-unique (`browser::NEXT_TAB_ID`), so at most one
    /// workspace matches.
    pub fn index_of_tab(&self, id: usize) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|w| w.tabs.lock().unwrap().tabs().iter().any(|t| t.id == id))
    }

    /// Live `WebView` of tab `id`, whichever workspace owns it.
    pub fn webview_of_tab(&self, id: usize) -> Option<&WebView> {
        self.workspaces.iter().find_map(|w| w.webviews.get(&id))
    }

    /// Workspace owning AI session `id` (process-unique as well — see
    /// `main::next_acp_session_id`). Reconnect timers and login pollers fire
    /// regardless of which workspace the user has switched to since.
    pub fn index_of_session(&self, id: u64) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|w| w.acp_sessions.iter().any(|s| s.id == id))
    }

    /// Positional access, for code that must act on a specific workspace
    /// rather than the active one — MCP commands carry their caller's
    /// workspace id and resolve it to an index once per command.
    pub fn at(&self, index: usize) -> &Workspace {
        &self.workspaces[index]
    }

    pub fn at_mut(&mut self, index: usize) -> &mut Workspace {
        &mut self.workspaces[index]
    }

    /// Mutable iteration over every workspace — used at startup to seed each
    /// one's `acp_sessions` (not just the active workspace's).
    pub fn list_mut(&mut self) -> &mut [Workspace] {
        &mut self.workspaces
    }

    /// Create a new isolated workspace, seed it with one tab (mirrors how
    /// the app opens a single home-page tab on cold start — see the
    /// session-restore fallback in `main.rs`), and make it active.
    pub fn create(&mut self, name: String, color: String) -> &Workspace {
        let ws = Workspace::new(
            new_id(),
            name,
            color,
            Some(random_data_store_id()),
            self.max_history,
        );
        ws.tabs.lock().unwrap().open(self.home_page.clone());
        self.workspaces.push(ws);
        self.active_index = self.workspaces.len() - 1;
        self.active()
    }

    pub fn switch(&mut self, id: &str) -> bool {
        match self.workspaces.iter().position(|w| w.id == id) {
            Some(idx) => {
                self.active_index = idx;
                true
            }
            None => false,
        }
    }

    pub fn rename(&mut self, id: &str, name: String) -> bool {
        match self.workspaces.iter_mut().find(|w| w.id == id) {
            Some(w) => {
                w.name = name;
                true
            }
            None => false,
        }
    }

    /// Drops the workspace. Refuses to remove the last remaining one.
    ///
    /// Stage 1 MVP: does not clean up the on-disk `WKWebsiteDataStore` for
    /// the removed workspace — it's simply orphaned (cookies/cache stay on
    /// disk, unreachable). Real cleanup needs
    /// `WebViewBuilderExtDarwin::fetch_data_store_identifiers` +
    /// `WKWebsiteDataStore.remove(forIdentifier:)`, deferred to a later stage.
    pub fn remove(&mut self, id: &str) -> bool {
        if self.workspaces.len() <= 1 {
            return false;
        }
        match self.workspaces.iter().position(|w| w.id == id) {
            Some(idx) => {
                self.workspaces.remove(idx);
                if self.active_index >= self.workspaces.len() {
                    self.active_index = self.workspaces.len() - 1;
                } else if idx < self.active_index {
                    self.active_index -= 1;
                }
                true
            }
            None => false,
        }
    }
}

/// 16 random bytes read straight from `/dev/urandom` — always present on
/// macOS. No UUID/rand crate: this is the one primitive both the workspace
/// id (formatted as a UUIDv4 string below) and the WKWebsiteDataStore
/// identifier are built from.
fn random_bytes16() -> [u8; 16] {
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("failed to read /dev/urandom for workspace id generation");
    buf
}

fn random_data_store_id() -> [u8; 16] {
    random_bytes16()
}

/// UUIDv4-formatted id for `Workspace::id` (not used as a data store
/// identifier — just needs to be unique and stable for lookups).
fn new_id() -> String {
    let mut b = random_bytes16();
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant 10xx
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}
