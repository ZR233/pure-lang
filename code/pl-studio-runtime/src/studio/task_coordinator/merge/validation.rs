use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::git::checked_git;
use crate::agent::worktree::same_worktree_path;
use crate::studio::task_coordinator::{
    AgentDelivery, TaskRun, ThreadExecutionStatus, WorkCompletionKind, WorkCompletionRecord,
    WorkCompletionStatus, WorkUnit, WorkUnitStatus,
};

pub(super) fn ensure_preflight_delivery_identity(
    task_run_id: &str,
    agent_id: &str,
    work_unit: &WorkUnit,
    completion: &WorkCompletionRecord,
    delivery: &AgentDelivery,
) -> Result<()> {
    let mut mismatches = Vec::new();
    if work_unit.task_run_id != task_run_id {
        mismatches.push("taskRunId");
    }
    if work_unit.executor_thread_id.as_deref() != Some(agent_id) {
        mismatches.push("agentId");
    }
    if work_unit.execution_status() != ThreadExecutionStatus::Completed
        || work_unit.status() != WorkUnitStatus::Approved
    {
        mismatches.push("delivery status");
    }
    if work_unit.attempt == 0 {
        mismatches.push("attempt");
    }
    if completion.task_run_id != task_run_id
        || completion.work_unit_id != work_unit.id
        || completion.executor_agent_id != agent_id
        || completion.kind != WorkCompletionKind::Delivery
        || completion.status != WorkCompletionStatus::Approved
    {
        mismatches.push("completion");
    }
    if !same_worktree_path(&delivery.worktree.path, &work_unit.worktree_path) {
        mismatches.push("worktree path");
    }
    if delivery.worktree.branch != work_unit.branch {
        mismatches.push("worktree branch");
    }
    if delivery.base_commit != work_unit.base_commit {
        mismatches.push("base commit");
    }
    if completion.head_commit.as_deref() != Some(delivery.head_commit.as_str())
        || completion.changed_files != delivery.changed_files
        || completion.worktree_path != delivery.worktree.path
        || completion.branch != delivery.worktree.branch
        || completion.base_commit != delivery.base_commit
    {
        mismatches.push("completion delivery");
    }
    if !mismatches.is_empty() {
        bail!(
            "agent delivery does not match the planner-owned approved completion: {}",
            mismatches.join(", ")
        );
    }
    Ok(())
}

pub(super) async fn validate_final_head(run: &TaskRun, expected_head: &str) -> Result<()> {
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

pub(super) async fn validate_repository_identity(
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
