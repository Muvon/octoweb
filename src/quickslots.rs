use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickSlot {
    pub url: String,
    pub title: String,
    /// Favicon as base64 data-URI (optional — may not be cached yet)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
}

/// 10 slots indexed 0–9, mapped to keys 1–9,0.
pub type QuickSlots = [Option<QuickSlot>; 10];

pub fn save(slots: &QuickSlots) {
    let path = slots_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string(slots) {
        Ok(s) => {
            let _ = fs::write(&path, s);
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize quickslots"),
    }
}

pub fn load() -> QuickSlots {
    let path = slots_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Build JSON array for the footer/newtab UI.
/// Each element: { slot: 0-9, url, title, favicon } or null.
pub fn to_json(slots: &QuickSlots) -> String {
    serde_json::to_string(slots).unwrap_or_else(|_| "[]".into())
}

fn slots_path() -> PathBuf {
    crate::config::base_dir()
        .join("octoweb")
        .join("quickslots.json")
}
