use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionData {
    pub tabs: Vec<String>,
    pub active_url: String,
}

pub fn save_session(tabs: &[String], active_url: &str) {
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

    #[allow(dead_code)]
    /// Build a search URL for the given query
    pub fn search_url(&self, query: &str) -> String {
        let encoded = url_encode(query);
        self.search_engine.replace("{}", &encoded)
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
        .join("config.toml")
}

/// Minimal percent-encoding for query strings (no external dep needed)
#[allow(dead_code)]
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}
