use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionData {
    /// Tabs — accepts both new `[{url, title}]` and legacy `["url"]` formats.
    #[serde(deserialize_with = "deserialize_tabs")]
    pub tabs: Vec<SessionTab>,
    pub active_url: String,
}

pub fn save_session(tabs: &[SessionTab], active_url: &str) {
    let data = SessionData {
        tabs: tabs.to_vec(),
        active_url: active_url.to_string(),
    };
    let path = session_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string(&data) {
        Ok(s) => {
            let _ = fs::write(&path, s);
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize session"),
    }
}

/// Backward-compatible deserializer: accepts `["url"]` (legacy) or `[{url, title}]` (new).
fn deserialize_tabs<'de, D>(deserializer: D) -> Result<Vec<SessionTab>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TabOrString {
        Tab(SessionTab),
        Url(String),
    }

    let items: Vec<TabOrString> = Vec::deserialize(deserializer)?;
    Ok(items
        .into_iter()
        .map(|item| match item {
            TabOrString::Tab(t) => t,
            TabOrString::Url(url) => SessionTab {
                url,
                title: String::new(),
            },
        })
        .collect())
}

pub fn load_session() -> Option<SessionData> {
    let path = session_path();
    let raw = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn session_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("session.json")
}

// ── Favicon cache ─────────────────────────────────────────────────────────────
// Maps domain (e.g. "github.com") → base64 data-URI ("data:image/png;base64,...")
// Persisted to disk so favicons are available instantly on next launch.

pub fn save_favicons(cache: &std::collections::HashMap<String, String>) {
    let path = favicons_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string(cache) {
        Ok(s) => {
            let _ = fs::write(&path, s);
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize favicon cache"),
    }
}

pub fn load_favicons() -> std::collections::HashMap<String, String> {
    let path = favicons_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn favicons_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("favicons.json")
}

// ── History persistence ────────────────────────────────────────────────────────
// Full browsing history (title, url, visited_at) persisted across sessions.
// Capped at max_history entries on load — oldest entries are dropped first.

pub fn save_history(entries: &[crate::browser::HistoryEntry]) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    match serde_json::to_string(entries) {
        Ok(s) => {
            if let Err(e) = fs::write(&tmp, &s) {
                tracing::warn!(error = %e, "Failed to write history tmp");
                return;
            }
            // Atomic rename — APFS guarantees this won't corrupt history.json on crash
            if let Err(e) = fs::rename(&tmp, &path) {
                tracing::warn!(error = %e, "Failed to rename history tmp");
            }
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize history"),
    }
}

pub fn load_history() -> Vec<crate::browser::HistoryEntry> {
    let path = history_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("history.json")
}

// ── Prompt history persistence ────────────────────────────────────────────────
// Inline AI edit (⌘⇧E) prompt history — MRU-first Vec<String>.

pub fn save_prompt_history(entries: &[String]) {
    let path = prompt_history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    match serde_json::to_string(entries) {
        Ok(s) => {
            if let Err(e) = fs::write(&tmp, &s) {
                tracing::warn!(error = %e, "Failed to write prompt history tmp");
                return;
            }
            if let Err(e) = fs::rename(&tmp, &path) {
                tracing::warn!(error = %e, "Failed to rename prompt history tmp");
            }
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize prompt history"),
    }
}

pub fn load_prompt_history() -> Vec<String> {
    let path = prompt_history_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn prompt_history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("prompt_history.json")
}

// ── AI sidebar prompt history persistence ─────────────────────────────────────
// AI assistant sidebar prompt history — same MRU-first Vec<String> pattern.
// Shared across all ACP sessions: a global pool that seeds new sessions and
// receives every successfully submitted prompt regardless of which session
// produced it.

pub fn save_ai_prompt_history(entries: &[String]) {
    let path = ai_prompt_history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    match serde_json::to_string(entries) {
        Ok(s) => {
            if let Err(e) = fs::write(&tmp, &s) {
                tracing::warn!(error = %e, "Failed to write AI prompt history tmp");
                return;
            }
            if let Err(e) = fs::rename(&tmp, &path) {
                tracing::warn!(error = %e, "Failed to rename AI prompt history tmp");
            }
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize AI prompt history"),
    }
}

pub fn load_ai_prompt_history() -> Vec<String> {
    let path = ai_prompt_history_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn ai_prompt_history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("ai_prompt_history.json")
}

// ── ACP chat history persistence ──────────────────────────────────────────────
// Persists every assistant session (id, title, tag, optional ACP resume id) and
// its message log (user prompts, agent responses with their tool runs, errors)
// so the sidebar can be restored verbatim across browser restarts. Inline
// images stay live-only; tool input/output payloads are capped at persist time
// so the history file stays small.

/// One tool run inside an agent turn. Persisted so the sidebar's steps group
/// can be rebuilt (and re-inspected) after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpToolRecord {
    pub kind: String,
    pub title: String,
    /// "completed" | "failed" | "running" (= interrupted by cancel/restart)
    pub status: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpMessage {
    /// "user" | "agent" | "error" | "ui"
    pub role: String,
    /// Raw text. Markdown for agent, plain otherwise. For role="ui" this
    /// holds the envelope file id (so replay can rebuild the bubble).
    pub text: String,
    /// Unix epoch milliseconds at the time the message was committed.
    #[serde(default)]
    pub ts: u64,
    /// For role="ui": the full A2UI envelope body so we can re-render the
    /// surface bubble on cold-start. None for all other roles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a2ui: Option<serde_json::Value>,
    /// Tool runs of this agent turn (role="agent" only) — drives the steps
    /// group on replay. Empty for other roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AcpToolRecord>,
    /// Wall-clock duration of the whole turn in ms (role="agent" only).
    #[serde(default)]
    pub turn_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSessionSnapshot {
    pub id: u64,
    pub title: String,
    pub tag: String,
    /// ACP-protocol session id reported by the agent on Connected. Passed back
    /// as `--resume <id>` on next launch so the agent restores its in-memory
    /// conversation context. None if the agent never connected successfully.
    #[serde(default)]
    pub acp_session_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<AcpMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AcpHistory {
    pub sessions: Vec<AcpSessionSnapshot>,
    /// Last-active session id. Restored as the foreground session.
    #[serde(default)]
    pub active_id: u64,
}

pub fn save_acp_history(history: &AcpHistory) {
    let path = acp_history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    match serde_json::to_string(history) {
        Ok(s) => {
            if let Err(e) = fs::write(&tmp, &s) {
                tracing::warn!(error = %e, "Failed to write ACP history tmp");
                return;
            }
            if let Err(e) = fs::rename(&tmp, &path) {
                tracing::warn!(error = %e, "Failed to rename ACP history tmp");
            }
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize ACP history"),
    }
}

pub fn load_acp_history() -> Option<AcpHistory> {
    let path = acp_history_path();
    let raw = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn acp_history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("acp_history.json")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    /// URL to load on startup
    pub home_page: String,
    /// Search engine URL — `{}` is replaced with the encoded query
    pub search_engine: String,
    /// Max history entries to keep in memory
    pub max_history: usize,
    /// Initial window width
    pub window_width: u32,
    /// Initial window height
    pub window_height: u32,
    /// Auto-hide inline AI edit modal after submitting (show loading cursor instead)
    #[serde(default)]
    pub ai_edit_auto_hide: bool,
    /// Max prompt history entries to keep for inline AI edit (⌘⇧E)
    #[serde(default = "default_max_prompt_history")]
    pub max_prompt_history: usize,
    /// Max prompt history entries to keep for AI sidebar assistant
    #[serde(default = "default_max_ai_prompt_history")]
    pub max_ai_prompt_history: usize,
    /// Max persisted messages to keep per ACP chat session — older messages
    /// are dropped from disk (FIFO) once a session exceeds this cap.
    #[serde(default = "default_max_acp_session_messages")]
    pub max_acp_session_messages: usize,
    /// Enable proactive background learning from browsing history
    #[serde(default = "default_true")]
    pub proactive_learning: bool,
    /// How often to run the learning agent, in minutes
    #[serde(default = "default_learning_interval")]
    pub learning_interval_min: u64,
    /// Use the legacy aggressive hibernation curve (sqrt scaling). When false
    /// (default) the modern-laptop friendly curve is used — tabs survive longer
    /// on 16 GB+ machines. Turn on for tight-RAM setups or eager reclamation.
    #[serde(default)]
    pub aggressive_hibernation: bool,
    /// Whether the first-run welcome toast has been shown. Internal flag.
    #[serde(default)]
    pub first_run_completed: bool,
}

fn default_max_prompt_history() -> usize {
    50
}

fn default_max_ai_prompt_history() -> usize {
    50
}

fn default_max_acp_session_messages() -> usize {
    500
}

fn default_true() -> bool {
    true
}

fn default_learning_interval() -> u64 {
    30
}

impl Default for Config {
    fn default() -> Self {
        Self {
            home_page: "https://www.google.com".into(),
            search_engine: "https://www.google.com/search?q={}".into(),
            max_history: 1000,
            window_width: 1280,
            window_height: 800,
            ai_edit_auto_hide: false,
            max_prompt_history: 50,
            max_ai_prompt_history: 50,
            max_acp_session_messages: 500,
            proactive_learning: true,
            learning_interval_min: 30,
            aggressive_hibernation: false,
            first_run_completed: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();

        if !path.exists() {
            let cfg = Config::default();
            cfg.save();
            return cfg;
        }

        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(path = %path.display(), error = %e, "Failed to read config");
                return Config::default();
            }
        };

        match toml::from_str(&raw) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!(path = %path.display(), error = %e, "Invalid config");
                Config::default()
            }
        }
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match toml::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = fs::write(&path, s) {
                    tracing::warn!(error = %e, "Failed to write config");
                }
            }
            Err(e) => tracing::warn!(error = %e, "Failed to serialize config"),
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("config.toml")
}
