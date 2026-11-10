//! Sandboxed working directories for Octoweb's Octomind subprocesses.
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
use std::path::{Path, PathBuf};

fn workspace_dir(shared: &Path, workspace_id: &str, role: &str) -> PathBuf {
	// A bare `octoweb` is the same agent: Octomind starts a domain's `chief` for it.
	if matches!(role, "octoweb:chief" | "octoweb") && workspace_id != crate::workspace::DEFAULT_WORKSPACE_ID {
		// Octomind keys lessons by cwd basename. Siblings keep the Default
		// sandbox from containing other workspaces while preserving its scope.
		shared
			.with_file_name("octoweb-workspaces")
			.join(format!("octoweb-{workspace_id}"))
	} else {
		shared.to_path_buf()
	}
}

/// Preserve Default's existing Octomind scope and give other assistant
/// workspaces their own cwd. All roles retain the same local tool setup.
pub fn prepare(workspace_id: &str, role: &str) -> anyhow::Result<PathBuf> {
	let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory is unavailable"))?;
	prepare_in(&home, workspace_id, role)
}

fn prepare_in(home: &Path, workspace_id: &str, role: &str) -> anyhow::Result<PathBuf> {
	let shared = home.join(".local/share/octoweb");
	let workspace = workspace_dir(&shared, workspace_id, role);
	remove_legacy_render_ui(&shared, home);
	prepare_dir(&shared, &home.join(".agents/tools"))?;
	if workspace != shared {
		prepare_dir(&workspace, &shared.join(".agents/tools"))?;
		for name in [".env", "AGENTS.md"] {
			let source = shared.join(name);
			if source.try_exists()? {
				link(&source, &workspace.join(name))?;
			}
		}
	}
	Ok(workspace)
}

fn prepare_dir(workspace: &Path, source_tools: &Path) -> anyhow::Result<()> {
	use anyhow::Context;
	let tools = workspace.join(".agents/tools");
	fs::create_dir_all(&tools).with_context(|| format!("Could not prepare agent tools in {}", tools.display()))?;
	link_tools(&tools, source_tools)?;
	Ok(())
}

/// Versions before `render_ui` moved onto the MCP server installed it as a
/// bash script polling a file queue — first at `~/.agents/tools/render_ui`,
/// later in this workspace's tools dir. Left in place, `link_tools`
/// mirrors the home copy in, octomind discovers it as a local tool next to
/// `octoweb:render_ui`, and the agent fails bootstrap on the duplicate tool
/// name. Delete every copy and both queues. A user's own `render_ui` is kept:
/// ours is recognised by its content.
fn remove_legacy_render_ui(workspace: &Path, home: &Path) {
	// Workspace entry first: it may be a symlink to the home copy, which is
	// only recognisable through the link while the target still exists.
	let scripts = [
		workspace.join(".agents/tools/render_ui"),
		home.join(".agents/tools/render_ui"),
	];
	for script in scripts {
		if !is_legacy_render_ui(&script) {
			continue;
		}
		match fs::remove_file(&script) {
			Ok(()) => tracing::info!(
				path = %script.display(),
				"removed legacy render_ui script — superseded by octoweb:render_ui"
			),
			Err(e) => tracing::warn!(
				path = %script.display(),
				error = %e,
				"legacy render_ui script could not be removed"
			),
		}
	}
	let _ = fs::remove_dir_all(workspace.join("a2ui"));
	let _ = fs::remove_dir_all(home.join(".local/share/a2ui"));
}

/// Every shipped version of the legacy script carried both markers: the A2UI
/// v0.9 description and a queue under `~/.local/share/`.
fn is_legacy_render_ui(path: &Path) -> bool {
	fs::read_to_string(path)
		.is_ok_and(|script| script.contains("A2UI v0.9") && script.contains("queue=\"${HOME}/.local/share/"))
}

/// Symlink `~/.agents/tools/*` into the workspace tools dir. Skips entries
/// that already exist; stale symlinks pointing at removed user tools are
/// cleaned up first.
fn link_tools(ws_dir: &Path, source_tools: &Path) -> anyhow::Result<()> {
	// Remove dangling symlinks left from previously deleted user tools.
	for ent in fs::read_dir(ws_dir)? {
		let p = ent?.path();
		if p.is_symlink() && !p.try_exists()? {
			match fs::remove_file(&p) {
				Ok(()) => {}
				Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
				Err(e) => return Err(e.into()),
			}
		}
	}

	let entries = match fs::read_dir(source_tools) {
		Ok(entries) => entries,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
		Err(e) => return Err(e.into()),
	};
	for ent in entries {
		let ent = ent?;
		link(&ent.path(), &ws_dir.join(ent.file_name()))?;
	}
	Ok(())
}

fn link(source: &Path, destination: &Path) -> anyhow::Result<()> {
	use anyhow::Context;
	// Several sessions may prepare the same workspace concurrently. Creating
	// the link atomically also preserves files the user already put here.
	match std::os::unix::fs::symlink(source, destination) {
		Ok(()) => Ok(()),
		Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
		Err(e) => Err(e).with_context(|| format!("Could not link {} to {}", destination.display(), source.display())),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn assistant_directories_have_distinct_lesson_scopes() {
		let shared = Path::new("/home/user/.local/share/octoweb");
		assert_eq!(workspace_dir(shared, "default", "octoweb:chief"), shared);
		let first = workspace_dir(shared, "first", "octoweb:chief");
		let second = workspace_dir(shared, "second", "octoweb:chief");
		assert_eq!(workspace_dir(shared, "first", "octoweb"), first);
		assert_eq!(first.file_name().unwrap(), "octoweb-first");
		assert_eq!(second.file_name().unwrap(), "octoweb-second");
		assert!(!first.starts_with(shared));
		assert!(!second.starts_with(shared));
		for role in ["octoweb:learning", "octoweb:editor", "octoweb:mail-reader", "developer"] {
			assert_eq!(workspace_dir(shared, "first", role), shared);
		}
	}

	#[test]
	fn workspace_preparation_keeps_tools_settings_and_files_aligned() {
		let home = std::env::temp_dir().join(format!(
			"octoweb-agent-workspace-{:x}",
			u128::from_ne_bytes(crate::workspace::random_bytes16())
		));
		let shared = home.join(".local/share/octoweb");
		let user_tools = home.join(".agents/tools");
		fs::create_dir_all(&user_tools).unwrap();
		fs::create_dir_all(shared.join(".agents/tools")).unwrap();
		fs::write(user_tools.join("custom"), "user tool").unwrap();
		fs::write(shared.join(".agents/tools/local"), "local tool").unwrap();
		fs::write(shared.join(".env"), "TEST_KEY=value").unwrap();
		fs::write(shared.join("AGENTS.md"), "Shared instructions").unwrap();

		let first = prepare_in(&home, "first", "octoweb:chief").unwrap();
		let second = prepare_in(&home, "second", "octoweb:chief").unwrap();
		for workspace in [&first, &second] {
			assert_eq!(
				fs::read_to_string(workspace.join(".agents/tools/custom")).unwrap(),
				"user tool"
			);
			assert_eq!(
				fs::read_to_string(workspace.join(".agents/tools/local")).unwrap(),
				"local tool"
			);
			assert_eq!(fs::read_link(workspace.join(".env")).unwrap(), shared.join(".env"));
			assert_eq!(
				fs::read_link(workspace.join("AGENTS.md")).unwrap(),
				shared.join("AGENTS.md")
			);
		}
		fs::write(first.join("draft.md"), "First workspace").unwrap();
		assert!(!second.join("draft.md").exists());
		fs::write(shared.join(".env"), "TEST_KEY=updated").unwrap();
		assert_eq!(fs::read_to_string(first.join(".env")).unwrap(), "TEST_KEY=updated");
		fs::remove_file(first.join("AGENTS.md")).unwrap();
		fs::write(first.join("AGENTS.md"), "Workspace instructions").unwrap();
		prepare_in(&home, "first", "octoweb:chief").unwrap();
		assert_eq!(
			fs::read_to_string(first.join("AGENTS.md")).unwrap(),
			"Workspace instructions"
		);
		fs::remove_file(user_tools.join("custom")).unwrap();
		prepare_in(&home, "first", "octoweb:chief").unwrap();
		assert!(first.join(".agents/tools/custom").symlink_metadata().is_err());

		let handles: Vec<_> = (0..4)
			.map(|_| {
				let home = home.clone();
				std::thread::spawn(move || prepare_in(&home, "parallel", "octoweb:chief").unwrap())
			})
			.collect();
		for handle in handles {
			handle.join().unwrap();
		}
		fs::create_dir_all(home.join(".local/share/octoweb-workspaces")).unwrap();
		fs::write(home.join(".local/share/octoweb-workspaces/octoweb-blocked"), "file").unwrap();
		assert!(prepare_in(&home, "blocked", "octoweb:chief")
			.unwrap_err()
			.to_string()
			.contains("Could not prepare agent tools"));
		fs::remove_dir_all(home).unwrap();
	}
}
