use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// Every workspace's slots, keyed by workspace id. Pins belong to the
/// workspace they were saved in, like tabs and cookies.
pub type AllQuickSlots = HashMap<String, QuickSlots>;

pub fn save_all(all: &AllQuickSlots) {
    let path = slots_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string(all) {
        Ok(s) => {
            let _ = fs::write(&path, s);
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize quickslots"),
    }
}

pub fn load_all() -> AllQuickSlots {
    let Ok(raw) = fs::read_to_string(slots_path()) else {
        return AllQuickSlots::new();
    };
    if let Ok(all) = serde_json::from_str::<AllQuickSlots>(&raw) {
        return all;
    }
    // Pre-workspaces format: a bare 10-slot array. Anything in it was pinned
    // before workspaces existed, so it belongs to "default".
    match serde_json::from_str::<QuickSlots>(&raw) {
        Ok(slots) => AllQuickSlots::from([("default".to_string(), slots)]),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse quickslots");
            AllQuickSlots::new()
        }
    }
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
