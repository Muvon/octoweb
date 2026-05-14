/// Polls `~/.local/share/a2ui/` for envelope files written by the
/// `render_ui` tool. Emits one snapshot per unique (id, status) transition
/// onto the tao event-loop proxy as `AppEvent::A2uiUpdate`.
///
/// Octoweb routes every envelope to the *currently active* sidebar
/// session — we don't try to bind surfaces to a specific octomind process.
/// Multi-session users with parallel agents will see surfaces appear in
/// whichever session is focused when the agent rendered them. Simple and
/// sufficient for the common case (one agent talking at a time).
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct A2uiSnapshot {
    pub id: String,
    pub status: String,
    /// Full body — main.rs forwards it to the sidebar as-is.
    pub body: serde_json::Value,
}

/// Spawns a polling thread that lives for the life of the process.
pub fn start<F>(dir: PathBuf, emit: F)
where
    F: Fn(A2uiSnapshot) + Send + 'static,
{
    let seen: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    std::thread::Builder::new()
        .name("a2ui-watcher".into())
        .spawn(move || {
            // Warm without firing — avoids spamming already-resolved surfaces
            // from previous sessions on cold start. Surfaces still `pending`
            // on disk DO get replayed, so live forms restore on app restart.
            warm(&dir, &seen);
            loop {
                scan(&dir, &seen, &emit);
                std::thread::sleep(Duration::from_millis(400));
            }
        })
        .expect("a2ui-watcher thread");
}

fn warm(dir: &PathBuf, seen: &Arc<Mutex<HashMap<String, String>>>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut map = seen.lock().unwrap();
    for ent in entries.flatten() {
        let path = ent.path();
        if !is_envelope(&path) {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
        let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("");
        if id.is_empty() || status.is_empty() {
            continue;
        }
        if status != "pending" {
            map.insert(id.to_string(), status.to_string());
        }
    }
}

fn scan<F>(dir: &PathBuf, seen: &Arc<Mutex<HashMap<String, String>>>, emit: &F)
where
    F: Fn(A2uiSnapshot),
{
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if !is_envelope(&path) {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let status = v
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || status.is_empty() {
            continue;
        }
        {
            let mut map = seen.lock().unwrap();
            if map.get(&id).map(String::as_str) == Some(status.as_str()) {
                continue;
            }
            map.insert(id.clone(), status.clone());
        }
        emit(A2uiSnapshot {
            id,
            status,
            body: v,
        });
    }
}

fn is_envelope(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".json") && !name.ends_with(".tmp.json")
}
