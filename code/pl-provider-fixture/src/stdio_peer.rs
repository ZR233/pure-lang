//! Shared helpers for the scenario-owned stdio peers (MCP and fake LSP).
//!
//! Both peers are started by the real product through its supervised worker and
//! must reclaim the whole subtree on exit. To make that observable, each peer
//! spawns a grandchild that ignores `SIGTERM` and lives in its own process group
//! (so a group-wide signal cannot reach it) and records its own and the
//! grandchild's pid/starttime into a coordination file. The coordinator then
//! OS-polls the real business pids.

use anyhow::{Context, Result};
// `fs` and `Path` are only exercised by the Linux `/proc` helpers below; gating
// them keeps the Windows build free of unused-import warnings under `-D warnings`.
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Spawns `sh -c 'trap "" TERM; exec sleep ...'` in its own process group.
///
/// The shell sets the `SIGTERM` disposition to ignore before `exec`, so the
/// `sleep` image inherits an ignored `SIGTERM` (a plain `SIGTERM` cannot stop
/// it) while `SIGKILL` still can. `process_group(0)` detaches it from the peer's
/// group, so a group-wide signal misses it — exactly the descendant a product
/// supervisor must adopt and reap by pid.
fn spawn_sigterm_ignoring_grandchild() -> Result<Child> {
    let shell = if cfg!(unix) { "/bin/sh" } else { "sh" };
    let mut command = Command::new(shell);
    command
        .args(["-c", "trap \"\" TERM; exec sleep 100000"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .spawn()
        .context("failed to spawn the scenario-owned SIGTERM-ignoring grandchild")
}

/// Confirms the grandchild is really alive before its pid is recorded.
#[cfg(target_os = "linux")]
fn ensure_grandchild_alive(pid: u32) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if Path::new(&format!("/proc/{pid}")).exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    anyhow::bail!("the scenario-owned grandchild {pid} did not stay alive")
}

#[cfg(not(target_os = "linux"))]
fn ensure_grandchild_alive(_pid: u32) -> Result<()> {
    Ok(())
}

/// Spawns the SIGTERM-ignoring grandchild, keeps its handle owned so it cannot
/// become a zombie, confirms it stayed alive, and returns its pid.
///
/// The peer never kills the grandchild itself: reclaiming it is exactly what the
/// product's supervision must do, so the acceptance can tell a self-supervised
/// subtree apart from a leaked one.
pub(crate) fn spawn_tracked_grandchild() -> Result<u32> {
    let grandchild = spawn_sigterm_ignoring_grandchild()?;
    let pid = grandchild.id();
    ensure_grandchild_alive(pid)?;
    std::thread::spawn(move || {
        let mut grandchild = grandchild;
        let _ = grandchild.wait();
    });
    Ok(pid)
}

/// The Linux process start time (jiffies since boot) for [pid], if alive.
///
/// Field 22 of `/proc/<pid>/stat`; the executable name may contain spaces and
/// parentheses, so parsing resumes after the last `)`.
#[cfg(target_os = "linux")]
pub(crate) fn proc_start_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = after_name.split_whitespace().collect();
    fields.get(19).and_then(|value| value.parse::<u64>().ok())
}

/// Non-Linux hosts have no `/proc` start-time baseline.
#[cfg(not(target_os = "linux"))]
pub(crate) fn proc_start_ticks(_pid: u32) -> Option<u64> {
    None
}
