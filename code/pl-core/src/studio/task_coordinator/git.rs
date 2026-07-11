use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RepositorySnapshot {
    pub(super) workspace_root: PathBuf,
    pub(super) git_common_dir: PathBuf,
    pub(super) branch: String,
    pub(super) head: String,
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
        let range = format!("{base_commit}..{head_commit}");
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
                &range,
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

fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("git {} failed: {error}", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
