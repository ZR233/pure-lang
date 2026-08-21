use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};

use super::TaskGitFingerprint;
use crate::agent::worktree::git_compatible_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepositorySnapshot {
    pub(crate) workspace_root: PathBuf,
    pub(crate) git_common_dir: PathBuf,
    pub(crate) branch: String,
    pub(crate) head: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorktreeChangeInspection {
    pub(super) head: String,
    pub(super) dirty: bool,
    pub(super) ahead_by: u32,
    pub(super) changed_file_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GitDiffSelection {
    All,
    ExcludeDesign,
}

const INITIAL_COMMIT_MESSAGE: &str = "chore: initialize Pure Studio workspace";
const MINIMUM_ABBREVIATED_COMMIT_LENGTH: usize = 7;
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

pub(crate) async fn inspect_repository(
    path: impl AsRef<Path>,
    require_clean: bool,
) -> Result<RepositorySnapshot> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || inspect_repository_blocking(&path, require_clean))
        .await
        .context("git repository inspection task failed")?
}

pub(crate) async fn fingerprint_repository(
    path: impl AsRef<Path>,
    base_commit: &str,
    expected_head: &str,
) -> Result<TaskGitFingerprint> {
    let path = path.as_ref().to_path_buf();
    let base_commit = base_commit.to_string();
    let expected_head = expected_head.to_string();
    tokio::task::spawn_blocking(move || {
        fingerprint_repository_blocking(&path, &base_commit, &expected_head)
    })
    .await
    .context("Git fingerprint task failed")?
}

pub(crate) async fn ensure_no_git_operation(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || ensure_no_git_operation_blocking(&path))
        .await
        .context("Git operation inspection task failed")?
}

pub(super) async fn changed_files_between(
    path: impl AsRef<Path>,
    base_commit: &str,
    head_commit: &str,
) -> Result<Vec<String>> {
    changed_files_between_selected(path, base_commit, head_commit, GitDiffSelection::All).await
}

pub(super) async fn changed_files_between_selected(
    path: impl AsRef<Path>,
    base_commit: &str,
    head_commit: &str,
    selection: GitDiffSelection,
) -> Result<Vec<String>> {
    let path = path.as_ref().to_path_buf();
    let base_commit = base_commit.to_string();
    let head_commit = head_commit.to_string();
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new("git");
        command.arg("-C").arg(&path).args([
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            "--find-copies",
            "--find-copies-harder",
            "--diff-filter=ACDMRTUXB",
            &base_commit,
            &head_commit,
        ]);
        append_diff_pathspec(&mut command, selection);
        crate::process::configure_background_std_command(&mut command);
        let output = command
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

pub(super) async fn diff_between(
    path: impl AsRef<Path>,
    base_commit: &str,
    head_commit: &str,
    selection: GitDiffSelection,
) -> Result<String> {
    let path = path.as_ref().to_path_buf();
    let base_commit = base_commit.to_string();
    let head_commit = head_commit.to_string();
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new("git");
        command.arg("-C").arg(&path).args([
            "diff",
            "--find-renames",
            "--find-copies",
            "--find-copies-harder",
            &base_commit,
            &head_commit,
        ]);
        append_diff_pathspec(&mut command, selection);
        crate::process::configure_background_std_command(&mut command);
        let output = command.output().context("failed to run git diff")?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!("git diff failed: {error}");
        }
        String::from_utf8(output.stdout).context("review diff is not UTF-8")
    })
    .await
    .context("git review diff task failed")?
}

fn append_diff_pathspec(command: &mut Command, selection: GitDiffSelection) {
    command.arg("--").arg(".");
    if selection == GitDiffSelection::ExcludeDesign {
        command.arg(":(exclude)design/**");
    }
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
        let mut command = Command::new("git");
        command.arg("-C").arg(&path).args([
            "merge-base",
            "--is-ancestor",
            &base_commit,
            &head_commit,
        ]);
        crate::process::configure_background_std_command(&mut command);
        let output = command
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

pub(super) async fn resolve_commit_oid(
    path: impl AsRef<Path>,
    abbreviated_oid: &str,
) -> Result<String> {
    let path = path.as_ref().to_path_buf();
    let abbreviated_oid = abbreviated_oid.trim().to_ascii_lowercase();
    if abbreviated_oid.len() < MINIMUM_ABBREVIATED_COMMIT_LENGTH
        || !abbreviated_oid.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!(
            "headCommit must be a hexadecimal commit id or an unambiguous abbreviation of at least {MINIMUM_ABBREVIATED_COMMIT_LENGTH} characters"
        );
    }
    tokio::task::spawn_blocking(move || {
        let commit_revision = format!("{abbreviated_oid}^{{commit}}");
        let resolved = git_output(
            &path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &commit_revision,
            ],
        )
        .with_context(|| {
            format!("headCommit `{abbreviated_oid}` is not an unambiguous commit id")
        })?;
        if !resolved.starts_with(&abbreviated_oid) {
            bail!("headCommit `{abbreviated_oid}` does not identify a commit by object id");
        }
        Ok(resolved)
    })
    .await
    .context("git commit resolution task failed")?
}

pub(super) async fn resolve_tree_oid(path: impl AsRef<Path>, commit: &str) -> Result<String> {
    let path = path.as_ref().to_path_buf();
    let revision = format!("{}^{{tree}}", commit.trim());
    tokio::task::spawn_blocking(move || {
        git_output(
            &path,
            &["rev-parse", "--verify", "--end-of-options", &revision],
        )
    })
    .await
    .context("Git tree resolution task failed")?
}

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
    let workspace_root =
        git_compatible_path(std::fs::canonicalize(&workspace_root).unwrap_or(workspace_root));
    let git_common_dir =
        git_compatible_path(std::fs::canonicalize(&common_dir).unwrap_or(common_dir));
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

fn fingerprint_repository_blocking(
    path: &Path,
    base_commit: &str,
    expected_head: &str,
) -> Result<TaskGitFingerprint> {
    let snapshot = inspect_repository_blocking(path, false)?;
    let index_diff = git_output_bytes(
        &snapshot.workspace_root,
        &["diff", "--cached", "--binary", "--no-ext-diff", "--"],
    )?;
    let working_tree_diff = git_output_bytes(
        &snapshot.workspace_root,
        &["diff", "--binary", "--no-ext-diff", "--"],
    )?;
    let untracked = git_output_bytes(
        &snapshot.workspace_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let mut untracked_facts = Vec::new();
    for path_bytes in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative =
            std::str::from_utf8(path_bytes).context("untracked Git path is not UTF-8")?;
        let object_id = git_hash_object(&snapshot.workspace_root, relative)?;
        untracked_facts.extend_from_slice(path_bytes);
        untracked_facts.push(0);
        untracked_facts.extend_from_slice(object_id.as_bytes());
        untracked_facts.push(0);
    }
    Ok(TaskGitFingerprint {
        workspace_root: normalized_path(&snapshot.workspace_root),
        git_common_dir: normalized_path(&snapshot.git_common_dir),
        branch: snapshot.branch,
        head: snapshot.head,
        base_commit: base_commit.to_string(),
        expected_head: expected_head.to_string(),
        operation: git_operation(&snapshot.workspace_root)?,
        index_diff_hash: pl_core::canonical_content_hash(&index_diff),
        working_tree_diff_hash: pl_core::canonical_content_hash(&working_tree_diff),
        untracked_content_hash: pl_core::canonical_content_hash(&untracked_facts),
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
    crate::process::configure_background_std_command(&mut command);
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
    ensure_no_git_operation_blocking(workspace_root)
        .context("task mode cannot start while a Git operation is in progress")
}

fn ensure_no_git_operation_blocking(workspace_root: &Path) -> Result<()> {
    for (name, marker) in [
        ("merge", "MERGE_HEAD"),
        ("rebase", "rebase-merge"),
        ("rebase", "rebase-apply"),
        ("cherry-pick", "CHERRY_PICK_HEAD"),
        ("revert", "REVERT_HEAD"),
        ("sequencer", "sequencer"),
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
            bail!("unfinished Git {name} operation");
        }
    }
    Ok(())
}

fn git_operation(workspace_root: &Path) -> Result<String> {
    let mut operations = Vec::new();
    for (name, marker) in [
        ("merge", "MERGE_HEAD"),
        ("rebase", "rebase-merge"),
        ("rebase", "rebase-apply"),
        ("cherry-pick", "CHERRY_PICK_HEAD"),
        ("revert", "REVERT_HEAD"),
        ("sequencer", "sequencer"),
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
        if marker_path.exists() && !operations.contains(&name) {
            operations.push(name);
        }
    }
    Ok(if operations.is_empty() {
        "none".to_string()
    } else {
        operations.join(",")
    })
}

fn normalized_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
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

fn git_output_bytes(path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = git_command(path, args)?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), git_error(&output));
    }
    Ok(output.stdout)
}

fn git_hash_object(path: &Path, relative: &str) -> Result<String> {
    let mut command = Command::new("git");
    command
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .arg("-C")
        .arg(path)
        .args(["hash-object", "--no-filters", "--"])
        .arg(relative);
    crate::process::configure_background_std_command(&mut command);
    let output = command
        .output()
        .with_context(|| format!("failed to hash untracked path {relative}"))?;
    if !output.status.success() {
        bail!(
            "git hash-object failed for untracked path {relative}: {}",
            git_error(&output)
        );
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn temporary_repository(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pure-task-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn repository_snapshot_paths_are_native_non_verbatim() {
        let repository = temporary_repository("repository-path");
        std::fs::create_dir_all(&repository).unwrap();

        let snapshot = prepare_repository_for_task_blocking(&repository).unwrap();

        assert!(
            !snapshot
                .workspace_root
                .to_string_lossy()
                .starts_with(r"\\?\")
        );
        assert!(
            !snapshot
                .git_common_dir
                .to_string_lossy()
                .starts_with(r"\\?\")
        );
        std::fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn repository_fingerprint_detects_dirty_index_worktree_and_untracked_drift() {
        let repository = temporary_repository("fingerprint");
        std::fs::create_dir_all(&repository).unwrap();
        let snapshot = prepare_repository_for_task_blocking(&repository).unwrap();
        let base = snapshot.head;
        let initial = fingerprint_repository_blocking(&repository, &base, &base).unwrap();

        std::fs::write(repository.join("untracked.txt"), "first").unwrap();
        let untracked = fingerprint_repository_blocking(&repository, &base, &base).unwrap();
        assert_ne!(
            untracked.untracked_content_hash,
            initial.untracked_content_hash
        );
        assert_eq!(untracked.index_diff_hash, initial.index_diff_hash);

        git_output(&repository, &["add", "--", "untracked.txt"]).unwrap();
        let indexed = fingerprint_repository_blocking(&repository, &base, &base).unwrap();
        assert_ne!(indexed.index_diff_hash, initial.index_diff_hash);
        assert_eq!(
            indexed.untracked_content_hash,
            initial.untracked_content_hash
        );

        std::fs::write(repository.join("untracked.txt"), "second").unwrap();
        let worktree = fingerprint_repository_blocking(&repository, &base, &base).unwrap();
        assert_ne!(
            worktree.working_tree_diff_hash,
            initial.working_tree_diff_hash
        );
        assert_eq!(worktree.workspace_root, initial.workspace_root);
        assert_eq!(worktree.git_common_dir, initial.git_common_dir);
        assert_eq!(worktree.branch, initial.branch);
        assert_eq!(worktree.head, initial.head);

        std::fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn repository_fingerprint_reports_an_unfinished_git_operation() {
        let repository = temporary_repository("git-operation");
        std::fs::create_dir_all(&repository).unwrap();
        let snapshot = prepare_repository_for_task_blocking(&repository).unwrap();
        let merge_head = git_output(&repository, &["rev-parse", "--git-path", "MERGE_HEAD"])
            .map(PathBuf::from)
            .unwrap();
        let merge_head = if merge_head.is_absolute() {
            merge_head
        } else {
            repository.join(merge_head)
        };
        std::fs::write(merge_head, &snapshot.head).unwrap();

        let fingerprint =
            fingerprint_repository_blocking(&repository, &snapshot.head, &snapshot.head).unwrap();

        assert_eq!(fingerprint.operation, "merge");
        std::fs::remove_dir_all(repository).unwrap();
    }

    #[tokio::test]
    async fn review_diff_selection_preserves_all_git_change_kinds_and_excludes_design_together() {
        let repository = temporary_repository("review-diff-selection");
        std::fs::create_dir_all(repository.join("src")).unwrap();
        std::fs::create_dir_all(repository.join("design")).unwrap();
        prepare_repository_for_task_blocking(&repository).unwrap();
        // CI runners have no global Git identity; the test commits below must
        // not depend on host configuration.
        git_output(&repository, &["config", "user.name", "Pure Studio"]).unwrap();
        git_output(&repository, &["config", "user.email", "pure-studio@local"]).unwrap();
        std::fs::write(
            repository.join("src/rename_old.rs"),
            "pub fn renamed() -> &'static str { \"rename source with enough unique content\" }\n",
        )
        .unwrap();
        std::fs::write(
            repository.join("src/copy_source.rs"),
            "pub fn copied() -> &'static str { \"copy source with enough unique content\" }\n",
        )
        .unwrap();
        std::fs::write(repository.join("src/delete.rs"), "pub fn deleted() {}\n").unwrap();
        std::fs::write(repository.join("src/binary.bin"), [0, 1, 2, 3, 0, 4]).unwrap();
        std::fs::write(repository.join("design/review.md"), "# Baseline\n").unwrap();
        git_output(&repository, &["add", "--all", "--", "."]).unwrap();
        git_output(&repository, &["commit", "-m", "test: add review baseline"]).unwrap();
        let base = git_output(&repository, &["rev-parse", "HEAD"]).unwrap();

        std::fs::rename(
            repository.join("src/rename_old.rs"),
            repository.join("src/rename_new.rs"),
        )
        .unwrap();
        std::fs::copy(
            repository.join("src/copy_source.rs"),
            repository.join("src/copy_new.rs"),
        )
        .unwrap();
        std::fs::remove_file(repository.join("src/delete.rs")).unwrap();
        std::fs::write(repository.join("src/binary.bin"), [0, 9, 8, 7, 0, 6]).unwrap();
        std::fs::write(repository.join("design/review.md"), "# Updated\n").unwrap();
        git_output(&repository, &["add", "--all", "--", "."]).unwrap();
        git_output(
            &repository,
            &["commit", "-m", "test: update review fixture"],
        )
        .unwrap();
        let head = git_output(&repository, &["rev-parse", "HEAD"]).unwrap();

        let all = changed_files_between_selected(&repository, &base, &head, GitDiffSelection::All)
            .await
            .unwrap();
        for path in [
            "design/review.md",
            "src/binary.bin",
            "src/copy_new.rs",
            "src/copy_source.rs",
            "src/delete.rs",
            "src/rename_new.rs",
            "src/rename_old.rs",
        ] {
            assert!(
                all.contains(&path.to_string()),
                "missing `{path}` from {all:?}"
            );
        }

        let integrated = changed_files_between_selected(
            &repository,
            &base,
            &head,
            GitDiffSelection::ExcludeDesign,
        )
        .await
        .unwrap();
        assert!(!integrated.iter().any(|path| path.starts_with("design/")));
        assert_eq!(integrated, all[1..]);
        let integrated_diff =
            diff_between(&repository, &base, &head, GitDiffSelection::ExcludeDesign)
                .await
                .unwrap();
        assert!(!integrated_diff.contains("design/review.md"));
        assert!(integrated_diff.contains("src/binary.bin"));
        assert!(integrated_diff.contains("src/delete.rs"));
        assert!(integrated_diff.contains("src/rename_new.rs"));

        std::fs::remove_dir_all(repository).unwrap();
    }
}
