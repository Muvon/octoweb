//! The sandboxed directory every octomind subprocess runs inside.
//!
//! `acp.rs` sets this as the agent's cwd and `--sandbox` root, so the agent's
//! filesystem writes stay here instead of landing in the user's home.
//! Octomind also discovers local tools at `<cwd>/.agents/tools/*` — running
//! inside our workspace would otherwise silently hide the tools the user
//! installed in their own `~/.agents/tools/`, so we symlink those in.
//!
//! A2UI surfaces do NOT go through here. `render_ui` is a tool on octoweb's
//! MCP server (see `mcp.rs`), so there is nothing to install for it.

use std::fs;
use std::path::PathBuf;

/// Octoweb's internal data root, and the cwd + `--sandbox` root the agent
/// runs inside. Derived from `~/.local/share` to match the project's existing
/// XDG-style layout.
pub fn workspace_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".local")
        .join("share")
        .join("octoweb")
}

/// Where octomind looks for local tools, given cwd = `workspace_dir()`.
fn tools_dir() -> PathBuf {
    workspace_dir().join(".agents").join("tools")
}

/// Create the workspace and mirror the user's own local tools into it.
/// Best-effort: a failure here only costs the user their custom tools, so we
/// log rather than propagate.
pub fn prepare() {
    if let Err(e) = fs::create_dir_all(tools_dir()) {
        tracing::warn!(error = %e, "agent workspace tools dir not created — user local tools unavailable");
        return;
    }
    remove_legacy_render_ui();
    link_user_tools();
}

/// Versions before `render_ui` moved onto the MCP server dropped a bash
/// script here and watched a file queue. Left in place it would still be
/// discovered as a local tool, shadow `octoweb:render_ui`, and block forever
/// polling a file nothing writes to. Delete it and its queue on first launch.
fn remove_legacy_render_ui() {
    let script = tools_dir().join("render_ui");
    // Only ours — a symlink here is the user's own tool, mirrored by
    // `link_user_tools`, and theirs to keep.
    if script.is_file() && !script.is_symlink() {
        match fs::remove_file(&script) {
            Ok(()) => {
                tracing::info!("removed legacy render_ui script — superseded by octoweb:render_ui")
            }
            Err(e) => tracing::warn!(error = %e, "legacy render_ui script could not be removed"),
        }
    }
    let _ = fs::remove_dir_all(workspace_dir().join("a2ui"));
}

/// Symlink `~/.agents/tools/*` into the workspace tools dir. Skips entries
/// that already exist; stale symlinks pointing at removed user tools are
/// cleaned up first.
fn link_user_tools() {
    let Some(home) = dirs::home_dir() else { return };
    let user_dir = home.join(".agents").join("tools");
    let ws_dir = tools_dir();

    // Remove dangling symlinks left from previously deleted user tools.
    if let Ok(entries) = fs::read_dir(&ws_dir) {
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_symlink() && fs::metadata(&p).is_err() {
                let _ = fs::remove_file(&p);
            }
        }
    }

    let Ok(entries) = fs::read_dir(&user_dir) else {
        return;
    };
    for ent in entries.flatten() {
        let src = ent.path();
        let Some(name) = src.file_name() else {
            continue;
        };
        let dst = ws_dir.join(name);
        if dst.symlink_metadata().is_ok() {
            continue;
        }
        if let Err(e) = std::os::unix::fs::symlink(&src, &dst) {
            tracing::warn!(src = %src.display(), error = %e, "user tool symlink failed");
        }
    }
}
