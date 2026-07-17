use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepositorySnapshot {
    pub(super) workspace_root: PathBuf,
    pub(super) git_common_dir: PathBuf,
    pub(super) branch: String,
    pub(super) head: String,
}

const INITIAL_COMMIT_MESSAGE: &str = "chore: initialize Pure Studio workspace";
pub(super) const STUDIO_GIT_NAME_CONFIG: &str = "user.name=Pure Studio";
pub(super) const STUDIO_GIT_EMAIL_CONFIG: &str = "user.email=pure-studio@local";
const TASK_RUNTIME_EXCLUDES: &[&str] = &[".pure/worktrees/", "target/pure/"];

pub(super) async fn prepare_repository_for_task(
    path: impl AsRef<Path>,
) -> Result<RepositorySnapshot> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let _preparation_guard = repository_preparation_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        prepare_repository_for_task_blocking(&path)
    })
    .await
    .context("git repository preparation task failed")?
}

pub(super) async fn inspect_repository(
    path: impl AsRef<Path>,
    require_clean: bool,
) -> Result<RepositorySnapshot> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || inspect_repository_blocking(&path, require_clean))
        .await
        .context("git repository inspection task failed")?
}

pub(super) async fn changed_files_between(
    path: impl AsRef<Path>,
    base_commit: &str,
    head_commit: &str,
) -> Result<Vec<String>> {
    let path = path.as_ref().to_path_buf();
    let base_commit = base_commit.to_string();
    let head_commit = head_commit.to_string();
    tokio::task::spawn_blocking(move || {
        let output = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args([
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                "--find-copies",
                "--find-copies-harder",
                "--diff-filter=ACDMRTUXB",
                &base_commit,
                &head_commit,
                "--",
            ])
            .output()
            .context("failed to run git diff --name-status")?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!("git diff --name-status failed: {error}");
        }
        let mut fields = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty());
        let mut changed_files = Vec::new();
        while let Some(status) = fields.next() {
            let status = std::str::from_utf8(status).context("git diff status is not UTF-8")?;
            let path_count = match status.as_bytes().first() {
                Some(b'R' | b'C') => 2,
                Some(b'A' | b'D' | b'M' | b'T' | b'U' | b'X' | b'B') => 1,
                _ => bail!("git diff returned unsupported status `{status}`"),
            };
            for _ in 0..path_count {
                let field = fields
                    .next()
                    .with_context(|| format!("git diff status `{status}` is missing a path"))?;
                let path = std::str::from_utf8(field)
                    .context("git diff path is not UTF-8")?
                    .replace('\\', "/");
                changed_files.push(path);
            }
        }
        changed_files.sort();
        changed_files.dedup();
        Ok(changed_files)
    })
    .await
    .context("git changed-file inspection task failed")?
}

pub(super) async fn is_ancestor(
    path: impl AsRef<Path>,
    base_commit: &str,
    head_commit: &str,
) -> Result<bool> {
    let path = path.as_ref().to_path_buf();
    let base_commit = base_commit.to_string();
    let head_commit = head_commit.to_string();
    tokio::task::spawn_blocking(move || {
        let output = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["merge-base", "--is-ancestor", &base_commit, &head_commit])
            .output()
            .context("failed to run git merge-base --is-ancestor")?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => {
                let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
                bail!("git merge-base --is-ancestor failed: {error}");
            }
        }
    })
    .await
    .context("git ancestry inspection task failed")?
}

fn inspect_repository_blocking(path: &Path, require_clean: bool) -> Result<RepositorySnapshot> {
    let workspace_root = PathBuf::from(git_output(path, &["rev-parse", "--show-toplevel"])?);
    let branch = git_output(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .context("task mode requires a named branch; detached HEAD is not supported")?;
    let head = git_output(path, &["rev-parse", "HEAD"])?;
    let common_dir_value = git_output(path, &["rev-parse", "--git-common-dir"])?;
    let common_dir = PathBuf::from(common_dir_value);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        workspace_root.join(common_dir)
    };
    let workspace_root = std::fs::canonicalize(&workspace_root).unwrap_or(workspace_root);
    let git_common_dir = std::fs::canonicalize(&common_dir).unwrap_or(common_dir);
    if require_clean {
        let status = git_output(
            &workspace_root,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?;
        if !status.is_empty() {
            bail!("task mode requires a clean working tree");
        }
    }
    Ok(RepositorySnapshot {
        workspace_root,
        git_common_dir,
        branch,
        head,
    })
}

fn prepare_repository_for_task_blocking(path: &Path) -> Result<RepositorySnapshot> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("task project path does not exist: {}", path.display()))?;
    if !metadata.is_dir() {
        bail!("task project path is not a directory: {}", path.display());
    }

    let repository_probe = git_command(path, &["rev-parse", "--show-toplevel"])?;
    if !repository_probe.status.success() {
        let error = git_error(&repository_probe);
        if path.join(".git").exists() || !error.contains("not a git repository") {
            bail!("git rev-parse --show-toplevel failed: {error}");
        }
        git_output(path, &["init", "-b", "main"])
            .context("failed to initialize task project as a Git repository")?;
    }

    let workspace_root = PathBuf::from(git_output(path, &["rev-parse", "--show-toplevel"])?);
    ensure_task_runtime_paths_excluded(&workspace_root)?;
    let branch = git_output(
        &workspace_root,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )
    .context("task mode requires a named branch; detached HEAD is not supported")?;
    let head_probe = git_command(&workspace_root, &["rev-parse", "--verify", "HEAD"])?;
    if !head_probe.status.success() {
        create_initial_commit(&workspace_root)?;
    }

    ensure_no_task_start_git_operation(&workspace_root)?;
    let snapshot = inspect_repository_blocking(&workspace_root, true)?;
    if snapshot.branch != branch {
        bail!("task project branch changed during Git repository preparation");
    }
    Ok(snapshot)
}

fn ensure_task_runtime_paths_excluded(workspace_root: &Path) -> Result<()> {
    let exclude_path = PathBuf::from(git_output(
        workspace_root,
        &["rev-parse", "--git-path", "info/exclude"],
    )?);
    let exclude_path = if exclude_path.is_absolute() {
        exclude_path
    } else {
        workspace_root.join(exclude_path)
    };
    let content = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    let missing = TASK_RUNTIME_EXCLUDES
        .iter()
        .copied()
        .filter(|exclude| !content.lines().map(str::trim).any(|line| line == *exclude))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    let parent = exclude_path
        .parent()
        .context("Git exclude path has no parent directory")?;
    std::fs::create_dir_all(parent).context("failed to create Git info directory")?;
    let mut updated = content;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    for exclude in missing {
        updated.push_str(exclude);
        updated.push('\n');
    }
    std::fs::write(&exclude_path, updated).context("failed to update Git private exclude file")
}

fn create_initial_commit(workspace_root: &Path) -> Result<()> {
    git_output(workspace_root, &["add", "--all", "--", "."])
        .context("failed to stage task project files for the initial Git commit")?;

    let has_name = git_optional_output(workspace_root, &["config", "--get", "user.name"])?
        .is_some_and(|value| !value.trim().is_empty());
    let has_email = git_optional_output(workspace_root, &["config", "--get", "user.email"])?
        .is_some_and(|value| !value.trim().is_empty());
    let mut command = Command::new("git");
    command
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .arg("-C")
        .arg(workspace_root);
    if !has_name {
        command.args(["-c", STUDIO_GIT_NAME_CONFIG]);
    }
    if !has_email {
        command.args(["-c", STUDIO_GIT_EMAIL_CONFIG]);
    }
    let output = command
        .args(["commit", "--allow-empty", "-m", INITIAL_COMMIT_MESSAGE])
        .output()
        .context("failed to run git commit for task project initialization")?;
    if !output.status.success() {
        bail!("initial Git commit failed: {}", git_error(&output));
    }
    Ok(())
}

fn ensure_no_task_start_git_operation(workspace_root: &Path) -> Result<()> {
    for (name, marker) in [
        ("merge", "MERGE_HEAD"),
        ("rebase", "rebase-merge"),
        ("rebase", "rebase-apply"),
    ] {
        let marker_path = PathBuf::from(git_output(
            workspace_root,
            &["rev-parse", "--git-path", marker],
        )?);
        let marker_path = if marker_path.is_absolute() {
            marker_path
        } else {
            workspace_root.join(marker_path)
        };
        if marker_path.exists() {
            bail!("task mode cannot start while a Git {name} is in progress");
        }
    }
    Ok(())
}

fn repository_preparation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let output = git_command(path, args)?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), git_error(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_optional_output(path: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = git_command(path, args)?;
    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }
    match output.status.code() {
        Some(1) => Ok(None),
        _ => bail!("git {} failed: {}", args.join(" "), git_error(&output)),
    }
}

fn git_command(path: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .arg("-C")
        .arg(path)
        .args(args)
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
