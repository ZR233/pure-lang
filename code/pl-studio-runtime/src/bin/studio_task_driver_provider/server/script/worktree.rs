//! fixture 工作区的 Git worktree 观测。

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub(super) struct TaskWorktree {
    pub(super) path: PathBuf,
    pub(super) branch: String,
}

pub(super) fn task_worktree(workspace: &Path) -> Result<TaskWorktree> {
    let output = git_output(workspace, &["worktree", "list", "--porcelain"])?;
    let mut path = None;
    for line in output.lines().chain(std::iter::once("")) {
        if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(value));
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch refs/heads/")
            && branch.starts_with("pure-task-")
        {
            return Ok(TaskWorktree {
                path: path.context("Task worktree entry has no path")?,
                branch: branch.to_string(),
            });
        }
    }
    bail!("Task worktree is absent from git worktree list")
}

pub(super) fn git_output(workspace: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(workspace).args(args);
    pl_core::process::configure_background_std_command(&mut command);
    let output = command
        .output()
        .with_context(|| format!("failed to execute git {:?}", args))?;
    if !output.status.success() {
        bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
