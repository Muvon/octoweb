/// `render_ui` tool installer + envelope resolution helpers.
///
/// Octomind's local-tool runner scans `.agents/tools/<name>` from its CWD —
/// which is `dirs::home_dir()` (set by `acp.rs` and intentionally unchanged
/// by this module). So we drop the script at `~/.agents/tools/render_ui`
/// with +x and any octomind subprocess octoweb spawns picks it up
/// automatically.
///
/// The script queues an A2UI v0.9 envelope into `~/.local/share/a2ui/<id>.json`
/// and, when `await_events` is non-empty, polls that same file for the
/// resolution before exiting. The watcher in `a2ui_watcher` forwards every
/// status transition to the sidebar; user clicks IPC back through
/// `resolve()` here, which flips the file's status to `resolved` —
/// unblocking the script.
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Bash body. Self-contained, no octoweb-specific env contract — anyone who
/// drops this script into `.agents/tools/` of an octomind workspace gets the
/// same A2UI v0.9 behavior. `container` defaults to HOSTNAME for downstream
/// tools that care; octoweb itself routes by active session, not container.
const RENDER_UI_SCRIPT: &str = r####"#!/usr/bin/env bash
# @description Render an interactive UI surface (A2UI v0.9) in the operator's
# browser and optionally BLOCK until an awaited event fires. Use for forms,
# approval cards, multi-step wizards, confirms, dashboards, lists, pickers.
# Returns {name,surfaceId,sourceComponentId,context,dataModel} when an
# awaited event fires; returns {ok:true} immediately if await_events is
# empty (fire-and-forget update). CRITICAL — common model mistakes that
# render empty: (1) use "component", NOT "type". (2) Components are a FLAT
# adjacency list; each has string "id"; parents reference children by id,
# NEVER inline. (3) Exactly ONE component must have id:"root". (4) Only
# emit components from the catalog below; others are dropped.
# Catalog (component values):
#   Card{child|children}, Column{children,gap?,align?},
#   Row{children,gap?,align?,justify?}, Spacer{size?}, Divider,
#   Text{text,muted?}, Heading{text,level?:1-4}, Markdown{text}, Image{src,alt?,width?,height?},
#   Button{text,kind?:primary|danger|warn|success,disabled?,checks?:[ValueRef],
#          action:{event:{name,context?}} | {openUrl:string}},
#   TextField{label?,placeholder?,type?:text|email|password|number|tel,multiline?,rows?,value:{path}},
#   CheckBox{label?,value:{path}},
#   Slider{label?,min?,max?,step?,value:{path}},
#   ChoicePicker{label?,value:{path},choices:[scalar | {label,value}]},
#   DateTimeInput{label?,mode?:date|datetime|time,min?,max?,value:{path}},
#   List{children:{path,componentId}}
# ValueRef — any prop accepts: literal | {"path":"/json/ptr"} (RFC 6901
# against dataModel) | {"call":"<fn>","args":{...}} (recursively-resolved).
# Functions:
#   required({value}), email({value}), numeric({value}),
#   regex({value,pattern}), length({value,min?,max?}), range({value,min?,max?}),
#   and({values}), or({values}), not({value}), eq({a,b}), neq({a,b}),
#   formatString({template,args}) — "{key}" interpolation,
#   formatDate({value,locale?}), formatNumber({value,decimals?,locale?}),
#   formatCurrency({value,currency?,locale?}), openUrl({url}) (http(s)/mailto).
# Use `checks` on a Button to gate its action — array of ValueRefs, each
# evaluated at click time. If any is falsy, the action is suppressed and
# the check's optional `message` is toast-shown. Two-way binding writes
# inputs back to their {path} on every change; the dataModel is included
# in every event payload sent to the agent. updateDataModel can pre-fill
# any field before render. Working example (single line — copy this shape):
# {"await_events":["approve","reject"],"messages":[{"createSurface":{"surfaceId":"r1","catalogId":"https://a2ui.org/specification/v0_9/basic_catalog.json","theme":{"primaryColor":"#4ea3ff"}}},{"updateDataModel":{"surfaceId":"r1","path":"/form","value":{"reason":"","urgent":false}}},{"updateComponents":{"surfaceId":"r1","components":[{"id":"root","component":"Card","child":"body"},{"id":"body","component":"Column","gap":10,"children":["t","d","f1","f2","actions"]},{"id":"t","component":"Heading","text":"Review change","level":2},{"id":"d","component":"Markdown","text":"```diff\n- old\n+ new\n```"},{"id":"f1","component":"TextField","label":"Reason","value":{"path":"/form/reason"}},{"id":"f2","component":"CheckBox","label":"Urgent","value":{"path":"/form/urgent"}},{"id":"actions","component":"Row","gap":8,"children":["ok","no"]},{"id":"ok","component":"Button","text":"Approve","kind":"primary","checks":[{"call":"required","args":{"value":{"path":"/form/reason"}},"message":"Reason is required"}],"action":{"event":{"name":"approve","context":{"reason":{"path":"/form/reason"},"urgent":{"path":"/form/urgent"}}}}},{"id":"no","component":"Button","text":"Reject","kind":"danger","action":{"event":{"name":"reject"}}}]}}]}
# @param *messages array A2UI v0.9 envelopes — each item is {createSurface:{...}} OR {updateComponents:{surfaceId,components:[{id,component,...props}]}} OR {updateDataModel:{surfaceId,path,value}} OR {deleteSurface:{surfaceId}}. See description for component shapes and the function registry.
# @param await_events array Event names (strings) that resolve the call. Empty/missing = fire-and-forget. Each must match a Button's action.event.name.

set -euo pipefail

input="$(cat)"

# Reject empty envelopes early — an envelope with no messages targets no
# surface and just creates an orphan "Loading…" bubble in the sidebar.
msg_count=$(jq -r '.messages // [] | length' <<<"$input" 2>/dev/null || echo 0)
if [[ "$msg_count" == "0" ]]; then
  echo '{"ok":false,"error":"render_ui requires at least one message — createSurface, updateComponents, updateDataModel, or deleteSurface (each with surfaceId)"}'
  exit 1
fi

queue="${HOME}/.local/share/a2ui"
mkdir -p "$queue"

rand_hex=$(od -An -N3 -tx1 /dev/urandom | tr -d ' \n')
id="u_$(date -u +%Y%m%d_%H%M%S)_${rand_hex}"
file="${queue}/${id}.json"
tmpfile="${file}.tmp"
created=$(date -u +%Y-%m-%dT%H:%M:%SZ)

jq -c \
  --arg id "$id" \
  --arg created "$created" \
  --arg container "${HOSTNAME:-unknown}" \
  --arg tool "${OCTOMIND_TOOL_NAME:-render_ui}" \
  --arg ppid "$PPID" \
  '{
    id: $id,
    tool: $tool,
    container: $container,
    parent_pid: ($ppid | tonumber? // 0),
    created_at: $created,
    status: "pending",
    messages: (.messages // []),
    await_events: (.await_events // []),
    resolution: null
  }' <<<"$input" > "$tmpfile"
mv "$tmpfile" "$file"

awaiting=$(jq -r '.await_events | length' "$file")

if [[ "$awaiting" == "0" ]]; then
  echo '{"ok":true,"id":"'"$id"'","mode":"fire-and-forget"}'
  exit 0
fi

>&2 echo "[render_ui] queued id=$id — waiting for one of: $(jq -c .await_events "$file")"

while :; do
  status=$(jq -r '.status' "$file" 2>/dev/null || echo pending)
  if [[ "$status" != "pending" ]]; then
    jq -c '.resolution // {name: .status, context: null, dataModel: null}' "$file"
    exit 0
  fi
  sleep 1
done
"####;

/// Where the script lives. Octomind is spawned with CWD = `dirs::home_dir()`
/// (acp.rs), so `<home>/.agents/tools/` is where it'll be discovered.
pub fn tool_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".agents")
        .join("tools")
        .join("render_ui")
}

/// Queue directory the script writes envelopes into. Watcher polls this.
pub fn queue_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".local")
        .join("share")
        .join("a2ui")
}

/// Idempotent install — writes the script if absent or content drifted, sets
/// +x. Pre-creates the queue dir so the watcher's first scan succeeds even
/// before the agent ever invokes the tool. Logs but never panics: if install
/// fails the rest of the chat still works, the agent just won't have
/// `render_ui` available.
pub fn install() -> std::io::Result<PathBuf> {
    let path = tool_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let needs_write = match fs::read_to_string(&path) {
        Ok(existing) => existing != RENDER_UI_SCRIPT,
        Err(_) => true,
    };
    if needs_write {
        let mut f = fs::File::create(&path)?;
        f.write_all(RENDER_UI_SCRIPT.as_bytes())?;
    }
    let mut perms = fs::metadata(&path)?.permissions();
    if perms.mode() & 0o111 != 0o111 {
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }
    let _ = fs::create_dir_all(queue_dir());
    Ok(path)
}

/// Flip a pending envelope's status to `resolved` and stamp the resolution
/// body the bash poll loop will print back to the agent. No-op only if the
/// file is already past `pending`. A click whose name isn't in
/// `await_events` still resolves the file — silently bailing would leave
/// the bash poll spinning forever, freezing the session in "thinking…"
/// (the JS optimistically locks the bubble + sets busy on click). Letting
/// the agent see the unexpected event name is strictly better than a
/// permanent hang: the agent can adapt or apologize.
pub fn resolve(file_id: &str, action: serde_json::Value) -> std::io::Result<()> {
    let path = queue_dir().join(format!("{file_id}.json"));
    let raw = fs::read_to_string(&path)?;
    let mut body: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if body.get("status").and_then(|s| s.as_str()) != Some("pending") {
        return Ok(());
    }
    let await_events = body
        .get("await_events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let action_name = action.get("name").and_then(|n| n.as_str()).unwrap_or("");
    if !await_events.is_empty() && !await_events.iter().any(|e| e.as_str() == Some(action_name)) {
        tracing::warn!(
            file_id = %file_id,
            action_name = %action_name,
            await_events = ?await_events,
            "A2UI button event not in await_events — resolving anyway to unblock the bash poll",
        );
    }
    let mut resolution = action;
    if let Some(obj) = resolution.as_object_mut() {
        obj.insert("actor".into(), serde_json::Value::String("human".into()));
        obj.insert(
            "resolved_at".into(),
            serde_json::Value::String(iso_utc_now()),
        );
    }
    body["status"] = serde_json::Value::String("resolved".into());
    body["resolution"] = resolution;
    let tmp = path.with_extension("tmp.json");
    fs::write(&tmp, serde_json::to_string(&body)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Drop envelopes older than `max_age_secs`. The script never tidies after
/// itself; without this the queue dir grows monotonically across sessions.
pub fn prune_old(dir: &Path, max_age_secs: u64) {
    let now = std::time::SystemTime::now();
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for ent in entries.flatten() {
        let Ok(meta) = ent.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        let Ok(elapsed) = now.duration_since(mtime) else {
            continue;
        };
        if elapsed.as_secs() > max_age_secs {
            let _ = fs::remove_file(ent.path());
        }
    }
}

fn iso_utc_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let tod = secs % 86400;
    let (y, m, d) = civil_from_days(days as i64);
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}
