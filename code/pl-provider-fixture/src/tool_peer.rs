//! Scenario-owned background tool used by the shutdown acceptance.
//!
//! The scripted model issues one `exec` call whose command runs this binary in
//! `--tool-peer` mode inside the session's isolated project workspace. The
//! product starts it through the real supervised tool worker and must reclaim
//! that whole subtree on exit. Like the MCP/LSP peers, this one spawns a
//! grandchild that ignores `SIGTERM` and lives in its own process group, so the
//! grandchild can only be reclaimed by the product's real descendant
//! supervision (a group-wide signal misses it), and records its own and the
//! grandchild's pid/starttime into a coordination file.
//!
//! The process deliberately keeps running (the command outlives the runtime's
//! foreground window), so the tool stays a live supervised task while the
//! coordinator forces the application exit.

use anyhow::{Context, Result};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

use crate::stdio_peer::{proc_start_ticks, spawn_tracked_grandchild};

/// Options for [`run_tool_peer`].
pub struct ToolPeerOptions {
    /// File the peer writes its own and the grandchild's pid/starttime into.
    pub coord_file: PathBuf,
}

/// Runs the scenario-owned background tool until the product kills the subtree.
pub fn run_tool_peer(options: ToolPeerOptions) -> Result<()> {
    // Fail loudly rather than recording a pid that is already gone: a missing
    // grandchild must not turn into a vacuous "reclamation" pass. The helper
    // keeps the grandchild handle owned so it cannot become a zombie and never
    // kills it here; reclaiming it is exactly what the product must do.
    let grandchild_pid = spawn_tracked_grandchild()?;
    let tool_pid = std::process::id();
    write_coord_record(&options.coord_file, tool_pid, grandchild_pid)?;
    // Keep the task alive well past the force-exit window; the stdout marker is
    // only a human-readable confirmation that the peer started.
    println!("shutdown tool peer running (pid {tool_pid}, grandchild {grandchild_pid})");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// Writes the peer and grandchild identities so the coordinator can OS-poll the
/// real business PIDs with a pid-reuse guard.
fn write_coord_record(coord_file: &Path, tool_pid: u32, grandchild_pid: u32) -> Result<()> {
    let record = json!({
        "schema": "anywork-tool-peer/1",
        "toolPid": tool_pid,
        "toolStartTicks": proc_start_ticks(tool_pid),
        "grandchildPid": grandchild_pid,
        "grandchildStartTicks": proc_start_ticks(grandchild_pid),
    });
    if let Some(parent) = coord_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(
        coord_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )?;
    serde_json::to_writer_pretty(&mut temporary, &record)?;
    temporary.persist(coord_file).map_err(|error| error.error)?;
    Ok(())
}
