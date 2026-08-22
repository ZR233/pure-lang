use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorktreeChangeInspection {
    pub(super) head: String,
    pub(super) dirty: bool,
    pub(super) ahead_by: u32,
    pub(super) changed_file_count: u32,
}

pub(super) const STUDIO_GIT_NAME_CONFIG: &str = "user.name=Pure Studio";
pub(super) const STUDIO_GIT_EMAIL_CONFIG: &str = "user.email=pure-studio@local";

/// Reads an executor worktree only for cleanup preview. These facts never gate Task state.
pub(super) async fn inspect_worktree_changes(
    path: impl AsRef<Path>,
    base_commit: &str,
) -> Result<WorktreeChangeInspection> {
    let path = path.as_ref().to_path_buf();
    let base_commit = base_commit.to_string();
    tokio::task::spawn_blocking(move || {
        let head = git_output(&path, &["rev-parse", "HEAD"])?;
        let status = git_output(
            &path,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?;
        let ahead_by = git_output(
            &path,
            &["rev-list", "--count", &format!("{base_commit}..{head}")],
        )?
        .parse::<u32>()
        .context("git rev-list returned an invalid commit count")?;
        let changed_file_count =
            git_output(&path, &["diff", "--name-only", &base_commit, &head, "--"])?
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as u32;
        Ok(WorktreeChangeInspection {
            head,
            dirty: !status.is_empty(),
            ahead_by,
            changed_file_count,
        })
    })
    .await
    .context("worktree cleanup preview task failed")?
}

fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let output = git_command(path, args)?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), git_error(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_command(path: &Path, args: &[&str]) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .arg("-C")
        .arg(path)
        .args(args);
    crate::process::configure_background_std_command(&mut command);
    command
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))
}

fn git_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}
