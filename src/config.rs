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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            home_page: "https://www.google.com".into(),
            search_engine: "https://www.google.com/search?q={}".into(),
            max_history: 1000,
            window_width: 1280,
            window_height: 800,
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
