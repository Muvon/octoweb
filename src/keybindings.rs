//! Configurable global keybindings.
//!
//! Single source of truth for every remappable global shortcut. The NSEvent
//! monitor in `main.rs` looks up `(modifiers, keycode) -> Action` against a
//! [`Keymap`] built from compiled-in defaults overlaid with the user's saved
//! overrides (`~/.config/octoweb/keybindings.json`). Remapping rebuilds the
//! keymap in place so changes apply on the very next keystroke — no restart.
//!
//! ## Representation
//! A chord is a set of modifier flags plus one physical key, serialized as a
//! canonical string like `"cmd+shift+p"`. The key half is a stable *token*
//! (e.g. `p`, `slash`, `comma`, `return`) — physical-position based, so it maps
//! cleanly to both a macOS virtual keycode (for native matching) and the JS
//! `KeyboardEvent.code` the settings UI records.
//!
//! Quickslot digits (⌘1–⌘9 / ⌘⇧1–⌘⇧9) and the contextual Esc-closes-find-bar
//! are intentionally NOT here — they're a fixed positional family handled as a
//! fallback in the monitor.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ── Modifier bitset ─────────────────────────────────────────────────────────
pub const MOD_CMD: u8 = 1 << 0;
pub const MOD_CTRL: u8 = 1 << 1;
pub const MOD_OPT: u8 = 1 << 2;
pub const MOD_SHIFT: u8 = 1 << 3;

/// A remappable global action. The identity that survives remapping — distinct
/// from `AppEvent` so context-dependent dispatch (sidebar vs tab, find vs
/// navigation) stays in one place in `main.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    CommandPalette,
    Sidebar,
    SidebarFullscreen,
    InlineEdit,
    UrlEdit,
    DevTools,
    Settings,
    Shortcuts,
    NewSession,
    CloseTab,
    Quit,
    PrevTab,
    NextTab,
    ScrollDown,
    ScrollUp,
    ScrollTop,
    ScrollBottom,
    Reload,
    Screenshot,
    ScreenshotFull,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Find,
    Fullscreen,
    TogglePin,
    ToggleWorkspaces,
    MoveTab,
    FollowLink,
    ReopenTab,
    Back,
    Forward,
    NewTab,
    StopLoad,
}

/// `(action, stable id, human label, group, default chord)`.
/// Order here is the display order in the settings panel and help overlay.
const ACTION_TABLE: &[(Action, &str, &str, &str, &str)] = &[
    // Tabs & navigation
    (
        Action::CloseTab,
        "close_tab",
        "Close tab / session",
        "Tabs & Navigation",
        "cmd+w",
    ),
    (
        Action::NewSession,
        "new_session",
        "New AI session",
        "Tabs & Navigation",
        "cmd+t",
    ),
    (
        Action::PrevTab,
        "prev_tab",
        "Previous tab",
        "Tabs & Navigation",
        "ctrl+p",
    ),
    (
        Action::NextTab,
        "next_tab",
        "Next tab",
        "Tabs & Navigation",
        "ctrl+n",
    ),
    (
        Action::Reload,
        "reload",
        "Reload page",
        "Tabs & Navigation",
        "cmd+r",
    ),
    (
        Action::Back,
        "back",
        "Back",
        "Tabs & Navigation",
        "cmd+bracketleft",
    ),
    (
        Action::Forward,
        "forward",
        "Forward",
        "Tabs & Navigation",
        "cmd+bracketright",
    ),
    (
        Action::NewTab,
        "new_tab",
        "New tab",
        "Tabs & Navigation",
        "cmd+n",
    ),
    (
        Action::StopLoad,
        "stop_load",
        "Stop loading",
        "Tabs & Navigation",
        "cmd+period",
    ),
    (
        Action::UrlEdit,
        "url_edit",
        "Edit address",
        "Tabs & Navigation",
        "cmd+e",
    ),
    // View
    (
        Action::ScrollDown,
        "scroll_down",
        "Scroll down",
        "View",
        "ctrl+d",
    ),
    (Action::ScrollUp, "scroll_up", "Scroll up", "View", "ctrl+u"),
    (
        Action::ScrollTop,
        "scroll_top",
        "Scroll to top",
        "View",
        "ctrl+t",
    ),
    (
        Action::ScrollBottom,
        "scroll_bottom",
        "Scroll to bottom",
        "View",
        "ctrl+b",
    ),
    (Action::ZoomIn, "zoom_in", "Zoom in", "View", "cmd+equal"),
    (Action::ZoomOut, "zoom_out", "Zoom out", "View", "cmd+minus"),
    (
        Action::ZoomReset,
        "zoom_reset",
        "Reset zoom",
        "View",
        "cmd+0",
    ),
    (
        Action::Fullscreen,
        "fullscreen",
        "Fullscreen window",
        "View",
        "cmd+return",
    ),
    // Tools
    (Action::Find, "find", "Find in page", "Tools", "cmd+f"),
    (
        Action::Screenshot,
        "screenshot",
        "Screenshot viewport",
        "Tools",
        "cmd+s",
    ),
    (
        Action::ScreenshotFull,
        "screenshot_full",
        "Full-page screenshot",
        "Tools",
        "cmd+shift+s",
    ),
    (
        Action::DevTools,
        "devtools",
        "Toggle DevTools",
        "Tools",
        "cmd+shift+i",
    ),
    // AI & panels
    (
        Action::CommandPalette,
        "command_palette",
        "Command palette",
        "AI & Panels",
        "cmd+shift+p",
    ),
    (
        Action::Sidebar,
        "sidebar",
        "Toggle AI sidebar",
        "AI & Panels",
        "cmd+shift+a",
    ),
    (
        Action::SidebarFullscreen,
        "sidebar_fullscreen",
        "Fullscreen AI sidebar",
        "AI & Panels",
        "cmd+shift+return",
    ),
    (
        Action::InlineEdit,
        "inline_edit",
        "AI edit selection",
        "AI & Panels",
        "cmd+shift+e",
    ),
    // App
    (
        Action::Shortcuts,
        "shortcuts",
        "Keyboard shortcuts",
        "App",
        "cmd+slash",
    ),
    (Action::Settings, "settings", "Settings", "App", "cmd+comma"),
    (Action::Quit, "quit", "Quit", "App", "cmd+q"),
    (
        Action::TogglePin,
        "toggle_pin",
        "Pin/unpin tab",
        "Tabs & Navigation",
        "cmd+shift+n",
    ),
    (
        Action::ToggleWorkspaces,
        "toggle_workspaces",
        "Workspaces",
        "Tabs & Navigation",
        "cmd+shift+o",
    ),
    (
        Action::MoveTab,
        "move_tab",
        "Move tab to workspace",
        "Tabs & Navigation",
        "cmd+shift+m",
    ),
    (
        Action::ReopenTab,
        "reopen_tab",
        "Reopen closed tab",
        "Tabs & Navigation",
        "cmd+shift+t",
    ),
    (
        Action::FollowLink,
        "follow_link",
        "Follow link on page",
        "Tools",
        "cmd+shift+f",
    ),
];

impl Action {
    /// Every remappable action, in display order. Test-only: production code
    /// walks `ACTION_TABLE` directly.
    #[cfg(test)]
    pub fn all() -> Vec<Action> {
        ACTION_TABLE.iter().map(|e| e.0).collect()
    }

    pub fn id(self) -> &'static str {
        ACTION_TABLE
            .iter()
            .find(|e| e.0 == self)
            .map(|e| e.1)
            .unwrap_or("")
    }
    pub fn label(self) -> &'static str {
        ACTION_TABLE
            .iter()
            .find(|e| e.0 == self)
            .map(|e| e.2)
            .unwrap_or("")
    }
    pub fn default_chord(self) -> &'static str {
        ACTION_TABLE
            .iter()
            .find(|e| e.0 == self)
            .map(|e| e.4)
            .unwrap_or("")
    }
    fn from_id(id: &str) -> Option<Action> {
        ACTION_TABLE.iter().find(|e| e.1 == id).map(|e| e.0)
    }
}

// ── Key token ⇄ macOS virtual keycode (kVK_ANSI_* from HIToolbox/Events.h) ───
// Physical-position based, ANSI layout. Used to translate stored tokens into the
// keycodes the NSEvent monitor sees. Also drives display-symbol rendering.
const TOKEN_KEYCODES: &[(&str, u16)] = &[
    ("a", 0),
    ("s", 1),
    ("d", 2),
    ("f", 3),
    ("h", 4),
    ("g", 5),
    ("z", 6),
    ("x", 7),
    ("c", 8),
    ("v", 9),
    ("b", 11),
    ("q", 12),
    ("w", 13),
    ("e", 14),
    ("r", 15),
    ("y", 16),
    ("t", 17),
    ("o", 31),
    ("u", 32),
    ("i", 34),
    ("p", 35),
    ("l", 37),
    ("j", 38),
    ("k", 40),
    ("n", 45),
    ("m", 46),
    ("1", 18),
    ("2", 19),
    ("3", 20),
    ("4", 21),
    ("5", 23),
    ("6", 22),
    ("7", 26),
    ("8", 28),
    ("9", 25),
    ("0", 29),
    ("equal", 24),
    ("minus", 27),
    ("bracketright", 30),
    ("bracketleft", 33),
    ("quote", 39),
    ("semicolon", 41),
    ("backslash", 42),
    ("comma", 43),
    ("slash", 44),
    ("period", 47),
    ("backquote", 50),
    ("return", 36),
    ("tab", 48),
    ("space", 49),
    ("escape", 53),
    ("left", 123),
    ("right", 124),
    ("down", 125),
    ("up", 126),
];

fn token_to_keycode(token: &str) -> Option<u16> {
    TOKEN_KEYCODES.iter().find(|e| e.0 == token).map(|e| e.1)
}

/// Display symbol for a key token (e.g. `slash` -> `/`, `return` -> `↵`).
fn key_symbol(token: &str) -> String {
    match token {
        "return" => "↵".into(),
        "tab" => "⇥".into(),
        "space" => "Space".into(),
        "escape" => "Esc".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "slash" => "/".into(),
        "comma" => ",".into(),
        "period" => ".".into(),
        "equal" => "=".into(),
        "minus" => "-".into(),
        "semicolon" => ";".into(),
        "quote" => "'".into(),
        "backslash" => "\\".into(),
        "bracketleft" => "[".into(),
        "bracketright" => "]".into(),
        "backquote" => "`".into(),
        t if t.len() == 1 => t.to_uppercase(),
        t => t.to_uppercase(),
    }
}

// ── Chord ───────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub mods: u8,
    pub token: String,
}

impl Chord {
    /// Parse `"cmd+shift+p"`. Rejects unknown key tokens and chords with no
    /// modifier (a bare letter would shadow ordinary typing globally).
    pub fn parse(s: &str) -> Result<Chord, String> {
        let mut mods = 0u8;
        let mut token: Option<String> = None;
        for part in s
            .split('+')
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty())
        {
            match part.as_str() {
                "cmd" | "meta" | "super" => mods |= MOD_CMD,
                "ctrl" | "control" => mods |= MOD_CTRL,
                "opt" | "alt" | "option" => mods |= MOD_OPT,
                "shift" => mods |= MOD_SHIFT,
                key => {
                    if token_to_keycode(key).is_none() {
                        return Err(format!("Unsupported key: {key}"));
                    }
                    if token.is_some() {
                        return Err("Only one non-modifier key allowed".into());
                    }
                    token = Some(key.to_string());
                }
            }
        }
        let token = token.ok_or("Press a key, not just modifiers")?;
        if mods == 0 {
            return Err("Add at least one modifier (⌘ ⌃ ⌥ ⇧)".into());
        }
        Ok(Chord { mods, token })
    }

    fn keycode(&self) -> Option<u16> {
        token_to_keycode(&self.token)
    }

    /// Modifier + key symbols in Apple's canonical order (⌃⌥⇧⌘ then key).
    pub fn symbols(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.mods & MOD_CTRL != 0 {
            v.push("⌃".into());
        }
        if self.mods & MOD_OPT != 0 {
            v.push("⌥".into());
        }
        if self.mods & MOD_SHIFT != 0 {
            v.push("⇧".into());
        }
        if self.mods & MOD_CMD != 0 {
            v.push("⌘".into());
        }
        v.push(key_symbol(&self.token));
        v
    }
}

// ── Keymap ────────────────────────────────────────────────────────────────—
/// Live, mutable binding state. Holds user overrides (action id -> chord string)
/// and a derived `(mods, keycode) -> Action` lookup table rebuilt on every edit.
pub struct Keymap {
    overrides: HashMap<String, String>,
    lookup: HashMap<(u8, u16), Action>,
}

impl Keymap {
    pub fn load() -> Keymap {
        let overrides = fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
            .unwrap_or_default();
        let mut km = Keymap {
            overrides,
            lookup: HashMap::new(),
        };
        km.rebuild();
        km
    }

    /// Effective chord string for an action: override if present, else default.
    fn effective(&self, action: Action) -> String {
        self.overrides
            .get(action.id())
            .cloned()
            .unwrap_or_else(|| action.default_chord().to_string())
    }

    fn effective_chord(&self, action: Action) -> Option<Chord> {
        Chord::parse(&self.effective(action)).ok()
    }

    fn rebuild(&mut self) {
        let mut lookup = HashMap::new();
        for &(action, ..) in ACTION_TABLE {
            if let Some(chord) = self.effective_chord(action) {
                if let Some(kc) = chord.keycode() {
                    // First writer wins, keeping defaults stable if an override
                    // somehow collides (set-time validation normally prevents it).
                    lookup.entry((chord.mods, kc)).or_insert(action);
                }
            }
        }
        self.lookup = lookup;
    }

    /// O(1) match for the NSEvent monitor.
    pub fn lookup(&self, mods: u8, keycode: u16) -> Option<Action> {
        self.lookup.get(&(mods, keycode)).copied()
    }

    /// The action currently bound to `chord`, if any (for conflict detection).
    fn action_for_chord(&self, chord: &Chord) -> Option<Action> {
        chord.keycode().and_then(|kc| self.lookup(chord.mods, kc))
    }

    /// Rebind `action` to `chord_str`. Rejects unparseable chords and chords
    /// already owned by a different action. On success rebuilds + persists.
    pub fn rebind(&mut self, action_id: &str, chord_str: &str) -> Result<(), String> {
        let action = Action::from_id(action_id).ok_or("Unknown action")?;
        let chord = Chord::parse(chord_str)?;
        if let Some(other) = self.action_for_chord(&chord) {
            if other != action {
                return Err(format!("Already used by “{}”", other.label()));
            }
        }
        let canonical = format!("{}{}", mod_prefix(chord.mods), chord.token);
        // Rebinding back to the compiled-in default just clears the override, so
        // the action is reported as "default" (no reset affordance shown).
        if canonical == action.default_chord() {
            self.overrides.remove(action.id());
        } else {
            self.overrides.insert(action.id().to_string(), canonical);
        }
        self.rebuild();
        self.persist();
        Ok(())
    }

    /// Restore one action to its default. Rejects if the default chord is held
    /// by another action's override (rebind that one first).
    pub fn reset(&mut self, action_id: &str) -> Result<(), String> {
        let action = Action::from_id(action_id).ok_or("Unknown action")?;
        if let Ok(def) = Chord::parse(action.default_chord()) {
            if let Some(other) = self.action_for_chord(&def) {
                if other != action {
                    return Err(format!("Default in use by “{}”", other.label()));
                }
            }
        }
        self.overrides.remove(action_id);
        self.rebuild();
        self.persist();
        Ok(())
    }

    pub fn reset_all(&mut self) {
        self.overrides.clear();
        self.rebuild();
        self.persist();
    }

    fn persist(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(&self.overrides) {
            Ok(s) => {
                let tmp = p.with_extension("json.tmp");
                if fs::write(&tmp, &s).is_ok() {
                    let _ = fs::rename(&tmp, &p);
                }
            }
            Err(e) => tracing::warn!(error = %e, "Failed to serialize keybindings"),
        }
    }

    /// Per-action binding data for the settings panel and help overlay.
    /// `{ actions: [{id,label,group,keys,is_default,default_keys}] }`
    pub fn ui_json(&self) -> serde_json::Value {
        let actions: Vec<serde_json::Value> = ACTION_TABLE
            .iter()
            .map(|&(action, id, label, group, default)| {
                let keys = self
                    .effective_chord(action)
                    .map(|c| c.symbols())
                    .unwrap_or_default();
                let default_keys = Chord::parse(default)
                    .map(|c| c.symbols())
                    .unwrap_or_default();
                let is_default = !self.overrides.contains_key(id);
                serde_json::json!({
                    "id": id,
                    "label": label,
                    "group": group,
                    "keys": keys,
                    "is_default": is_default,
                    "default_keys": default_keys,
                })
            })
            .collect();
        serde_json::json!({ "actions": actions })
    }
}

/// Canonical modifier prefix in fixed order (`cmd+ctrl+opt+shift+`).
fn mod_prefix(mods: u8) -> String {
    let mut s = String::new();
    if mods & MOD_CMD != 0 {
        s.push_str("cmd+");
    }
    if mods & MOD_CTRL != 0 {
        s.push_str("ctrl+");
    }
    if mods & MOD_OPT != 0 {
        s.push_str("opt+");
    }
    if mods & MOD_SHIFT != 0 {
        s.push_str("shift+");
    }
    s
}

fn path() -> PathBuf {
    crate::config::base_dir()
        .join("octoweb")
        .join("keybindings.json")
}
