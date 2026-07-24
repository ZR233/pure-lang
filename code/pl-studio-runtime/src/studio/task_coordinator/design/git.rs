use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use super::super::git::{
    STUDIO_GIT_EMAIL_CONFIG, STUDIO_GIT_NAME_CONFIG, changed_files_between, inspect_repository,
};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
pub(super) struct ExactRepositoryScope<'a> {
    pub(super) workspace_root: &'a Path,
    pub(super) git_common_dir: &'a Path,
    pub(super) branch: &'a str,
    pub(super) head: &'a str,
}

impl ExactRepositoryScope<'_> {
    pub(super) fn matches(&self, snapshot: &super::super::git::RepositorySnapshot) -> bool {
        normalized_path(&snapshot.workspace_root) == normalized_path(self.workspace_root)
            && normalized_path(&snapshot.git_common_dir) == normalized_path(self.git_common_dir)
            && snapshot.branch == self.branch
            && snapshot.head == self.head
    }
}

pub(super) async fn git_path_is_ignored(workspace: &Path, path: &str) -> Result<bool> {
    let output = run_git(workspace, &["check-ignore", "--no-index", "-q", "--", path]).await?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "git check-ignore failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

pub(super) async fn stage_paths(workspace: &Path, paths: &[String]) -> Result<()> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    run_git_checked(workspace, &args).await.map(|_| ())
}

pub(super) async fn collect_worktree_changes(workspace: &Path) -> Result<Vec<String>> {
    let mut changed = BTreeSet::new();
    changed.extend(git_paths(workspace, &["diff", "--name-only", "-z"]).await?);
    changed.extend(git_paths(workspace, &["diff", "--cached", "--name-only", "-z"]).await?);
    changed.extend(
        git_paths(
            workspace,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )
        .await?,
    );
    Ok(changed.into_iter().collect())
}

pub(super) async fn collect_unstaged_and_untracked(workspace: &Path) -> Result<Vec<String>> {
    let mut changed = BTreeSet::new();
    changed.extend(git_paths(workspace, &["diff", "--name-only", "-z"]).await?);
    changed.extend(
        git_paths(
            workspace,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )
        .await?,
    );
    Ok(changed.into_iter().collect())
}

pub(super) async fn cached_changed_files(workspace: &Path) -> Result<Vec<String>> {
    git_paths(workspace, &["diff", "--cached", "--name-only", "-z"]).await
}

pub(super) async fn read_head(workspace: &Path) -> Result<String> {
    let output = run_git_checked(workspace, &["rev-parse", "HEAD"]).await?;
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

pub(super) async fn write_tree(workspace: &Path) -> Result<String> {
    let output = run_git_checked(workspace, &["write-tree"]).await?;
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

pub(super) async fn read_tree(workspace: &Path, revision: &str) -> Result<String> {
    let tree_revision = format!("{revision}^{{tree}}");
    let output = run_git_checked(workspace, &["rev-parse", &tree_revision]).await?;
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

pub(super) async fn validate_exact_commit(
    workspace: &Path,
    commit: &str,
    previous_head: &str,
    expected_paths: &[String],
    expected_tree: &str,
) -> Result<Vec<String>> {
    ensure_single_parent(workspace, commit, previous_head).await?;
    let committed_tree = read_tree(workspace, commit).await?;
    if committed_tree != expected_tree {
        bail!(
            "commit tree does not match the validated staged tree: expected {expected_tree}, actual {committed_tree}"
        );
    }
    let changed_files = changed_files_between(workspace, previous_head, commit).await?;
    let actual = changed_files.iter().collect::<BTreeSet<_>>();
    let expected = expected_paths.iter().collect::<BTreeSet<_>>();
    if actual != expected {
        bail!(
            "commit diff does not match the validated design paths: expected {:?}, actual {:?}",
            expected_paths,
            changed_files
        );
    }
    Ok(changed_files)
}

pub(super) async fn git_paths(workspace: &Path, args: &[&str]) -> Result<Vec<String>> {
    let output = run_git_checked(workspace, args).await?;
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .context("Git returned a non-UTF-8 path")
                .map(|path| path.replace('\\', "/"))
        })
        .collect()
}

pub(super) async fn ensure_single_parent(
    workspace: &Path,
    commit: &str,
    parent: &str,
) -> Result<()> {
    let output = run_git_checked(workspace, &["rev-list", "--parents", "-n", "1", commit]).await?;
    let line = String::from_utf8(output.stdout)?.trim().to_string();
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.as_slice() != [commit, parent] {
        bail!("design commit is not the only commit after the task base");
    }
    Ok(())
}

pub(super) async fn compensate_commit(
    scope: ExactRepositoryScope<'_>,
    previous_head: &str,
) -> Result<()> {
    let snapshot = inspect_repository(scope.workspace_root, true).await?;
    if !scope.matches(&snapshot) {
        bail!("cannot compensate commit because the exact task repository scope changed");
    }
    run_git_checked(scope.workspace_root, &["reset", "--mixed", previous_head]).await?;
    run_git_checked(
        scope.workspace_root,
        &[
            "restore",
            "--source",
            previous_head,
            "--worktree",
            "--",
            ".",
        ],
    )
    .await
    .map(|_| ())
}

pub(super) async fn ensure_repository_clean(workspace: &Path) -> Result<()> {
    inspect_repository(workspace, true).await.map(|_| ())
}

pub(super) async fn run_git_checked(workspace: &Path, args: &[&str]) -> Result<Output> {
    let output = run_git(workspace, args).await?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

pub(super) async fn run_git(workspace: &Path, args: &[&str]) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(workspace)
        .args([
            "-c",
            STUDIO_GIT_NAME_CONFIG,
            "-c",
            STUDIO_GIT_EMAIL_CONFIG,
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    crate::process::configure_background_command(&mut command);
    Ok(tokio::time::timeout(GIT_TIMEOUT, command.output())
        .await
        .with_context(|| format!("git {} timed out", args.join(" ")))??)
}

pub(super) fn normalized_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}
