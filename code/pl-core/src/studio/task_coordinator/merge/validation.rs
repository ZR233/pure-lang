use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::git::{checked_git, run_git};
use crate::studio::task_coordinator::{
    AgentDelivery, AgentOutcomeRecord, AgentOutcomeStatus, BranchLeaseRecord, TaskRunRecord,
    WorkUnitRecord, WorkUnitStatus,
};

pub(super) struct MergePreflight {
    pub(super) workspace: PathBuf,
    pub(super) pre_index_tree: String,
}

pub(super) fn ensure_preflight_delivery_identity(
    task_run_id: &str,
    agent_id: &str,
    work_unit: &WorkUnitRecord,
    outcome: &AgentOutcomeRecord,
    delivery: &AgentDelivery,
) -> Result<()> {
    if outcome.task_run_id != task_run_id
        || outcome.agent_id != agent_id
        || outcome.owner_path != "/root"
        || outcome.initiated_by != "planner"
        || outcome.role != "executor"
        || outcome.status != AgentOutcomeStatus::Completed
        || outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
        || work_unit.task_run_id != task_run_id
        || work_unit.agent_id.as_deref() != Some(agent_id)
        || work_unit.status != WorkUnitStatus::Delivered
        || work_unit.attempt != outcome.attempt
        || !(1..=3).contains(&work_unit.attempt)
        || delivery.worktree.path != work_unit.worktree_path
        || delivery.worktree.branch != work_unit.branch
        || delivery.base_commit != work_unit.base_commit
    {
        bail!("agent delivery does not match the planner-owned delivered work unit");
    }
    Ok(())
}

pub(super) async fn validate_merge_preflight(
    run: &TaskRunRecord,
    lease: &BranchLeaseRecord,
    work_unit: &WorkUnitRecord,
    delivery: &AgentDelivery,
    caller_expected_head: &str,
) -> Result<MergePreflight> {
    if run.expected_head != caller_expected_head || lease.expected_head != caller_expected_head {
        bail!("caller expectedHeadCommit does not match the durable branch head");
    }
    if lease.task_run_id != run.id
        || lease.branch != run.branch
        || normalized_path(Path::new(&lease.git_common_dir))
            != normalized_path(Path::new(&run.git_common_dir))
    {
        bail!("TaskRun and BranchLease do not describe the same repository branch");
    }
    let workspace = PathBuf::from(&run.workspace_root);
    validate_repository_identity(
        &workspace,
        Path::new(&run.workspace_root),
        Path::new(&run.git_common_dir),
        &run.branch,
        caller_expected_head,
        true,
    )
    .await?;
    let worktree = PathBuf::from(&work_unit.worktree_path);
    validate_repository_identity(
        &worktree,
        Path::new(&work_unit.worktree_path),
        Path::new(&run.git_common_dir),
        &work_unit.branch,
        &delivery.head_commit,
        true,
    )
    .await?;
    if delivery.worktree.path != work_unit.worktree_path
        || delivery.worktree.branch != work_unit.branch
        || delivery.base_commit != work_unit.base_commit
    {
        bail!("delivery no longer matches the assigned executor worktree");
    }
    let ancestry = run_git(
        &worktree,
        vec![
            "merge-base".into(),
            "--is-ancestor".into(),
            delivery.base_commit.clone(),
            delivery.head_commit.clone(),
        ],
    )
    .await?;
    if !ancestry.success {
        bail!("delivery HEAD no longer descends from its recorded base");
    }
    let changed_files =
        changed_files_between(&worktree, &delivery.base_commit, &delivery.head_commit).await?;
    if changed_files != delivery.changed_files {
        bail!("executor changed files no longer match the validated delivery scope");
    }
    let pre_index_tree = checked_git(&workspace, vec!["write-tree".into()]).await?;
    Ok(MergePreflight {
        workspace,
        pre_index_tree,
    })
}

pub(super) async fn validate_final_head(run: &TaskRunRecord, expected_head: &str) -> Result<()> {
    validate_repository_identity(
        Path::new(&run.workspace_root),
        Path::new(&run.workspace_root),
        Path::new(&run.git_common_dir),
        &run.branch,
        expected_head,
        true,
    )
    .await
}

pub(super) async fn changed_files_between(
    repository: &Path,
    base: &str,
    head: &str,
) -> Result<Vec<String>> {
    let output = run_git(
        repository,
        vec![
            "diff".into(),
            "--name-status".into(),
            "-z".into(),
            "--find-renames".into(),
            "--find-copies".into(),
            "--find-copies-harder".into(),
            "--diff-filter=ACDMRTUXB".into(),
            base.into(),
            head.into(),
            "--".into(),
        ],
    )
    .await?;
    if !output.success {
        bail!("git diff --name-status failed: {}", output.stderr_lossy());
    }
    parse_name_status(&output.stdout)
}

async fn validate_repository_identity(
    repository: &Path,
    expected_workspace: &Path,
    expected_common_dir: &Path,
    expected_branch: &str,
    expected_head: &str,
    require_clean: bool,
) -> Result<()> {
    let workspace = checked_git(
        repository,
        vec!["rev-parse".into(), "--show-toplevel".into()],
    )
    .await?;
    let workspace = PathBuf::from(workspace);
    let common_dir = checked_git(
        repository,
        vec!["rev-parse".into(), "--git-common-dir".into()],
    )
    .await?;
    let common_dir = resolve_git_path(repository, &common_dir)?;
    let branch = checked_git(
        repository,
        vec![
            "symbolic-ref".into(),
            "--quiet".into(),
            "--short".into(),
            "HEAD".into(),
        ],
    )
    .await?;
    let head = checked_git(repository, vec!["rev-parse".into(), "HEAD".into()]).await?;
    if normalized_path(&workspace) != normalized_path(expected_workspace)
        || normalized_path(&common_dir) != normalized_path(expected_common_dir)
        || branch != expected_branch
        || head != expected_head
    {
        bail!("Git repository identity or HEAD changed outside the task coordinator");
    }
    if require_clean {
        let status = checked_git(
            repository,
            vec![
                "status".into(),
                "--porcelain=v1".into(),
                "--untracked-files=all".into(),
            ],
        )
        .await?;
        if !status.is_empty() {
            bail!("Git workspace must be clean before task merge");
        }
    }
    Ok(())
}

fn parse_name_status(bytes: &[u8]) -> Result<Vec<String>> {
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut paths = Vec::new();
    while let Some(status) = fields.next() {
        let status = std::str::from_utf8(status).context("git diff status is not UTF-8")?;
        let count = match status.as_bytes().first() {
            Some(b'R' | b'C') => 2,
            Some(b'A' | b'D' | b'M' | b'T' | b'U' | b'X' | b'B') => 1,
            _ => bail!("unsupported git diff status `{status}`"),
        };
        for _ in 0..count {
            let path = fields
                .next()
                .with_context(|| format!("git diff status `{status}` has no path"))?;
            paths.push(
                std::str::from_utf8(path)
                    .context("git diff path is not UTF-8")?
                    .replace('\\', "/"),
            );
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn resolve_git_path(repository: &Path, value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        repository.join(path)
    };
    std::fs::canonicalize(path).context("failed to canonicalize Git common directory")
}

fn normalized_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}
