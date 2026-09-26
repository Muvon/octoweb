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

/// Move a pin to its new shortcut position, shifting intervening slots.
pub fn reorder(slots: &mut QuickSlots, from: usize, to: usize) -> bool {
    if from >= slots.len() || to >= slots.len() || from == to || slots[from].is_none() {
        return false;
    }
    if from < to {
        slots[from..=to].rotate_left(1);
    } else {
        slots[to..=from].rotate_right(1);
    }
    true
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_preserves_pins_and_updates_shortcut_positions() {
        let mut slots: QuickSlots = Default::default();
        for (index, name) in [(0, "A"), (1, "B"), (2, "C")] {
            slots[index] = Some(QuickSlot {
                url: name.into(),
                title: name.into(),
                favicon: Some(name.into()),
            });
        }
        assert!(reorder(&mut slots, 0, 2));
        assert_eq!(slots[0].as_ref().unwrap().url, "B");
        assert_eq!(slots[1].as_ref().unwrap().url, "C");
        assert_eq!(slots[2].as_ref().unwrap().favicon.as_deref(), Some("A"));
        assert!(reorder(&mut slots, 2, 9));
        assert!(slots[2].is_none());
        assert_eq!(slots[9].as_ref().unwrap().url, "A");
        assert!(reorder(&mut slots, 9, 0));
        assert_eq!(slots[0].as_ref().unwrap().url, "A");
        assert_eq!(slots.iter().flatten().count(), 3);
        let saved = to_json(&slots);
        assert!(!reorder(&mut slots, 3, 0));
        assert!(!reorder(&mut slots, 0, 10));
        assert!(!reorder(&mut slots, usize::MAX, 0));
        assert!(!reorder(&mut slots, 0, 0));
        assert_eq!(to_json(&slots), saved);
        let restored: QuickSlots = serde_json::from_str(&saved).unwrap();
        assert_eq!(to_json(&restored), saved);
    }
}
